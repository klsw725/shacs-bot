use crate::config::{ProviderConfig, ProvidersConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderSpec {
    pub name: &'static str,
    pub keywords: &'static [&'static str],
    pub env_key: Option<&'static str>,
    pub display_name: &'static str,
    pub backend: &'static str,
    pub env_extras: &'static [(&'static str, &'static str)],
    pub is_gateway: bool,
    pub is_local: bool,
    pub detect_by_key_prefix: Option<&'static str>,
    pub detect_by_base_keyword: Option<&'static str>,
    pub default_api_base: Option<&'static str>,
    pub strip_model_prefix: bool,
    pub supports_max_completion_tokens: bool,
    pub model_overrides: &'static [(&'static str, &'static str)],
    pub is_oauth: bool,
    pub is_direct: bool,
    pub supports_prompt_caching: bool,
    pub thinking_style: Option<&'static str>,
    pub reasoning_as_content: bool,
    pub supports_image_generation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderMatch {
    pub provider_id: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    specs: Vec<ProviderSpec>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            specs: provider_specs().to_vec(),
        }
    }

    pub fn specs(&self) -> &[ProviderSpec] {
        &self.specs
    }

    pub fn find_by_name(&self, name: &str) -> Option<&ProviderSpec> {
        find_by_name_in(&self.specs, name)
    }

    pub fn match_provider(
        &self,
        requested_provider: &str,
        model: &str,
        providers: &ProvidersConfig,
    ) -> Option<ProviderMatch> {
        if requested_provider != "auto" {
            let spec = self.find_by_name(requested_provider)?;
            return Some(ProviderMatch {
                provider_id: spec.name.to_owned(),
                model: strip_model_prefix(spec, model),
            });
        }

        if let Some((prefix, _)) = model.split_once('/') {
            if let Some(spec) = self.find_by_name(prefix) {
                if provider_is_usable(spec, providers.get(spec.name)) {
                    return Some(ProviderMatch {
                        provider_id: spec.name.to_owned(),
                        model: strip_model_prefix(spec, model),
                    });
                }
            }
        }

        let normalized_model = normalize_name(model);
        for spec in &self.specs {
            if !provider_is_usable(spec, providers.get(spec.name)) {
                continue;
            }
            if spec
                .keywords
                .iter()
                .any(|keyword| normalized_model.contains(&normalize_name(keyword)))
            {
                return Some(ProviderMatch {
                    provider_id: spec.name.to_owned(),
                    model: strip_model_prefix(spec, model),
                });
            }
        }

        let mut first_configured_local = None;
        for spec in &self.specs {
            if !spec.is_local {
                continue;
            }
            let Some(config) = providers.get(spec.name) else {
                continue;
            };
            let Some(api_base) = &config.api_base else {
                continue;
            };
            first_configured_local.get_or_insert(spec);
            let base_matches = match spec.detect_by_base_keyword {
                Some(keyword) => api_base.contains(keyword),
                None => true,
            };
            if base_matches {
                return Some(ProviderMatch {
                    provider_id: spec.name.to_owned(),
                    model: strip_model_prefix(spec, model),
                });
            }
        }
        if let Some(spec) = first_configured_local {
            return Some(ProviderMatch {
                provider_id: spec.name.to_owned(),
                model: strip_model_prefix(spec, model),
            });
        }

        for spec in &self.specs {
            if spec.is_oauth {
                continue;
            }
            if providers
                .get(spec.name)
                .is_some_and(|config| provider_is_usable(spec, Some(config)))
            {
                return Some(ProviderMatch {
                    provider_id: spec.name.to_owned(),
                    model: strip_model_prefix(spec, model),
                });
            }
        }

        None
    }
}

pub fn find_by_name(name: &str) -> Option<&'static ProviderSpec> {
    let normalized = normalize_name(name);
    provider_specs()
        .iter()
        .find(|spec| normalize_name(spec.name) == normalized)
}

pub fn provider_specs() -> &'static [ProviderSpec] {
    PROVIDERS
}

fn find_by_name_in<'a>(specs: &'a [ProviderSpec], name: &str) -> Option<&'a ProviderSpec> {
    let normalized = normalize_name(name);
    specs
        .iter()
        .find(|spec| normalize_name(spec.name) == normalized)
}

fn normalize_name(name: &str) -> String {
    name.chars()
        .map(|character| match character {
            '-' | ' ' | '.' => '_',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn provider_is_usable(spec: &ProviderSpec, config: Option<&ProviderConfig>) -> bool {
    if spec.is_oauth || spec.is_local || spec.is_direct {
        return config.is_some();
    }
    config
        .and_then(|config| config.api_key.as_deref())
        .is_some_and(|key| !key.is_empty())
}

fn strip_model_prefix(spec: &ProviderSpec, model: &str) -> String {
    if spec.strip_model_prefix {
        model
            .rsplit_once('/')
            .map_or_else(|| model.to_owned(), |(_, rest)| rest.to_owned())
    } else {
        model.to_owned()
    }
}

const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        is_direct: true,
        ..provider("custom", &[], None, "Custom", "openai_compat")
    },
    ProviderSpec {
        is_direct: true,
        ..provider(
            "azure_openai",
            &["azure", "azure-openai"],
            None,
            "Azure OpenAI",
            "azure_openai",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_key_prefix: Some("sk-or-"),
        detect_by_base_keyword: Some("openrouter"),
        default_api_base: Some("https://openrouter.ai/api/v1"),
        supports_prompt_caching: true,
        supports_image_generation: true,
        ..provider(
            "openrouter",
            &["openrouter"],
            Some("OPENROUTER_API_KEY"),
            "OpenRouter",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_key_prefix: Some("hf_"),
        detect_by_base_keyword: Some("huggingface"),
        default_api_base: Some("https://router.huggingface.co/v1"),
        ..provider(
            "huggingface",
            &["huggingface", "hugging-face"],
            Some("HF_TOKEN"),
            "Hugging Face",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_base_keyword: Some("aihubmix"),
        default_api_base: Some("https://aihubmix.com/v1"),
        strip_model_prefix: true,
        ..provider(
            "aihubmix",
            &["aihubmix"],
            Some("OPENAI_API_KEY"),
            "AiHubMix",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_base_keyword: Some("siliconflow"),
        default_api_base: Some("https://api.siliconflow.cn/v1"),
        ..provider(
            "siliconflow",
            &["siliconflow"],
            Some("OPENAI_API_KEY"),
            "SiliconFlow",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_base_keyword: Some("volces"),
        default_api_base: Some("https://ark.cn-beijing.volces.com/api/v3"),
        thinking_style: Some("thinking_type"),
        ..provider(
            "volcengine",
            &["volcengine", "volces", "ark"],
            Some("OPENAI_API_KEY"),
            "VolcEngine",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        default_api_base: Some("https://ark.cn-beijing.volces.com/api/coding/v3"),
        strip_model_prefix: true,
        thinking_style: Some("thinking_type"),
        ..provider(
            "volcengine_coding_plan",
            &["volcengine-plan"],
            Some("OPENAI_API_KEY"),
            "VolcEngine Coding Plan",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        detect_by_base_keyword: Some("bytepluses"),
        default_api_base: Some("https://ark.ap-southeast.bytepluses.com/api/v3"),
        strip_model_prefix: true,
        thinking_style: Some("thinking_type"),
        ..provider(
            "byteplus",
            &["byteplus"],
            Some("OPENAI_API_KEY"),
            "BytePlus",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_gateway: true,
        default_api_base: Some("https://ark.ap-southeast.bytepluses.com/api/coding/v3"),
        strip_model_prefix: true,
        thinking_style: Some("thinking_type"),
        ..provider(
            "byteplus_coding_plan",
            &["byteplus-plan"],
            Some("OPENAI_API_KEY"),
            "BytePlus Coding Plan",
            "openai_compat",
        )
    },
    ProviderSpec {
        supports_prompt_caching: true,
        ..provider(
            "anthropic",
            &["anthropic", "claude"],
            Some("ANTHROPIC_API_KEY"),
            "Anthropic",
            "anthropic",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.openai.com/v1"),
        supports_max_completion_tokens: true,
        supports_image_generation: true,
        ..provider(
            "openai",
            &["openai", "gpt"],
            Some("OPENAI_API_KEY"),
            "OpenAI",
            "openai_compat",
        )
    },
    ProviderSpec {
        detect_by_base_keyword: Some("codex"),
        default_api_base: Some("https://chatgpt.com/backend-api"),
        is_oauth: true,
        supports_image_generation: true,
        ..provider(
            "openai_codex",
            &["openai-codex"],
            None,
            "OpenAI Codex",
            "openai_codex",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.githubcopilot.com"),
        strip_model_prefix: true,
        supports_max_completion_tokens: true,
        is_oauth: true,
        ..provider(
            "github_copilot",
            &["github_copilot", "copilot"],
            None,
            "Github Copilot",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.deepseek.com"),
        thinking_style: Some("thinking_type"),
        ..provider(
            "deepseek",
            &["deepseek"],
            Some("DEEPSEEK_API_KEY"),
            "DeepSeek",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://generativelanguage.googleapis.com/v1beta/openai/"),
        ..provider(
            "gemini",
            &["gemini", "gemma"],
            Some("GEMINI_API_KEY"),
            "Gemini",
            "openai_compat",
        )
    },
    ProviderSpec {
        env_extras: &[("ZHIPUAI_API_KEY", "{api_key}")],
        default_api_base: Some("https://open.bigmodel.cn/api/paas/v4"),
        ..provider(
            "zhipu",
            &["zhipu", "glm", "zai"],
            Some("ZAI_API_KEY"),
            "Zhipu AI",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
        thinking_style: Some("enable_thinking"),
        ..provider(
            "dashscope",
            &["qwen", "dashscope"],
            Some("DASHSCOPE_API_KEY"),
            "DashScope",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.moonshot.ai/v1"),
        model_overrides: &[
            ("kimi-k2.5", "{\"temperature\":1.0}"),
            ("kimi-k2.6", "{\"temperature\":1.0}"),
        ],
        ..provider(
            "moonshot",
            &["moonshot", "kimi"],
            Some("MOONSHOT_API_KEY"),
            "Moonshot",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.minimax.io/v1"),
        thinking_style: Some("reasoning_split"),
        ..provider(
            "minimax",
            &["minimax"],
            Some("MINIMAX_API_KEY"),
            "MiniMax",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.minimax.io/anthropic"),
        ..provider(
            "minimax_anthropic",
            &["minimax_anthropic"],
            Some("MINIMAX_API_KEY"),
            "MiniMax (Anthropic)",
            "anthropic",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.mistral.ai/v1"),
        ..provider(
            "mistral",
            &["mistral"],
            Some("MISTRAL_API_KEY"),
            "Mistral",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.stepfun.com/v1"),
        reasoning_as_content: true,
        ..provider(
            "stepfun",
            &["stepfun", "step"],
            Some("STEPFUN_API_KEY"),
            "Step Fun",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.xiaomimimo.com/v1"),
        ..provider(
            "xiaomi_mimo",
            &["xiaomi_mimo", "mimo"],
            Some("XIAOMIMIMO_API_KEY"),
            "Xiaomi MIMO",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_local: true,
        ..provider(
            "vllm",
            &["vllm"],
            Some("HOSTED_VLLM_API_KEY"),
            "vLLM/Local",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_local: true,
        detect_by_base_keyword: Some("11434"),
        default_api_base: Some("http://localhost:11434/v1"),
        ..provider(
            "ollama",
            &["ollama", "nemotron"],
            Some("OLLAMA_API_KEY"),
            "Ollama",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_local: true,
        detect_by_base_keyword: Some("1234"),
        default_api_base: Some("http://localhost:1234/v1"),
        ..provider(
            "lm_studio",
            &["lm-studio", "lmstudio", "lm_studio"],
            Some("LM_STUDIO_API_KEY"),
            "LM Studio",
            "openai_compat",
        )
    },
    ProviderSpec {
        is_direct: true,
        is_local: true,
        default_api_base: Some("http://localhost:8000/v3"),
        ..provider(
            "ovms",
            &["openvino", "ovms"],
            None,
            "OpenVINO Model Server",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://api.groq.com/openai/v1"),
        ..provider(
            "groq",
            &["groq"],
            Some("GROQ_API_KEY"),
            "Groq",
            "openai_compat",
        )
    },
    ProviderSpec {
        default_api_base: Some("https://qianfan.baidubce.com/v2"),
        ..provider(
            "qianfan",
            &["qianfan", "ernie"],
            Some("QIANFAN_API_KEY"),
            "Qianfan",
            "openai_compat",
        )
    },
];

const fn provider(
    name: &'static str,
    keywords: &'static [&'static str],
    env_key: Option<&'static str>,
    display_name: &'static str,
    backend: &'static str,
) -> ProviderSpec {
    ProviderSpec {
        name,
        keywords,
        env_key,
        display_name,
        backend,
        env_extras: &[],
        is_gateway: false,
        is_local: false,
        detect_by_key_prefix: None,
        detect_by_base_keyword: None,
        default_api_base: None,
        strip_model_prefix: false,
        supports_max_completion_tokens: false,
        model_overrides: &[],
        is_oauth: false,
        is_direct: false,
        supports_prompt_caching: false,
        thinking_style: None,
        reasoning_as_content: false,
        supports_image_generation: false,
    }
}
