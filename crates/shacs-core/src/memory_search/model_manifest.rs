/// The model `memory_search` uses unless the configuration chooses another.
pub const DEFAULT_MEMORY_MODEL_ID: &str = "dragonkue/multilingual-e5-small-ko-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelArchitecture {
    Bert,
    XlmRoberta,
}

/// How the token vectors of one input become its embedding, before L2 normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pooling {
    /// Mean of the token vectors the attention mask keeps.
    Mean,
    /// The vector of the first token.
    Cls,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    /// Path inside the model repository, `/` separated.
    pub name: &'static str,
    pub size: u64,
    /// Lowercase hex SHA-256 of the whole file.
    pub sha256: &'static str,
}

/// One supported embedding model, pinned to a repository revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelManifest {
    pub id: &'static str,
    pub revision: &'static str,
    pub architecture: ModelArchitecture,
    pub dimension: usize,
    pub query_prefix: &'static str,
    pub passage_prefix: &'static str,
    pub pooling: Pooling,
    /// Inputs are cut to this many tokens, special tokens included.
    pub max_tokens: usize,
    pub files: [ModelFile; 6],
}

pub const CONFIG_FILE: &str = "config.json";
pub const TOKENIZER_FILE: &str = "tokenizer.json";
pub const WEIGHTS_FILE: &str = "model.safetensors";

/// Values checked on 2026-10-31 against the Hugging Face API (LFS sha256) and files downloaded
/// at these revisions.
pub const MODEL_MANIFESTS: [ModelManifest; 2] = [
    ModelManifest {
        id: DEFAULT_MEMORY_MODEL_ID,
        revision: "fcfc26bf355882620c48df58be112275bd756f50",
        architecture: ModelArchitecture::Bert,
        dimension: 384,
        query_prefix: "query: ",
        passage_prefix: "passage: ",
        pooling: Pooling::Mean,
        max_tokens: 512,
        files: [
            ModelFile {
                name: CONFIG_FILE,
                size: 628,
                sha256: "d99cc57db7529a5b6921f38d899dafad5bb829e21078900c8ba2c4ae9d141acb",
            },
            ModelFile {
                name: TOKENIZER_FILE,
                size: 17_082_800,
                sha256: "cd98e5698b201ba914efb8c18b6709fa8735ab71dcad8d2b431e52e8bf68d932",
            },
            ModelFile {
                name: WEIGHTS_FILE,
                size: 470_637_416,
                sha256: "9794f247caf80caf54b5a04d391c932ba6260cf20bd16fba3892ce2d8f5784ec",
            },
            ModelFile {
                name: "1_Pooling/config.json",
                size: 296,
                sha256: "a19c83805e1ce4174f3fbfec4ac8d3b8dbae0c958f8fd51b80937eb33e0c5335",
            },
            ModelFile {
                name: "modules.json",
                size: 349,
                sha256: "84e40c8e006c9b1d6c122e02cba9b02458120b5fb0c87b746c41e0207cf642cf",
            },
            ModelFile {
                name: "sentence_bert_config.json",
                size: 53,
                sha256: "ec8e29d6dcb61b611b7d3fdd2982c4524e6ad985959fa7194eacfb655a8d0d51",
            },
        ],
    },
    ModelManifest {
        id: "nlpai-lab/KURE-v1",
        revision: "8b418a58414668e75532ed045c22d9ca018ae2b2",
        architecture: ModelArchitecture::XlmRoberta,
        dimension: 1024,
        query_prefix: "",
        passage_prefix: "",
        pooling: Pooling::Cls,
        max_tokens: 512,
        files: [
            ModelFile {
                name: CONFIG_FILE,
                size: 807,
                sha256: "852d42e020c7f989c2acaf30fc683b7f768e8c6d1ab17166e835442162bd825d",
            },
            ModelFile {
                name: TOKENIZER_FILE,
                size: 17_083_053,
                sha256: "fb3c3b93c46fd5a8634e262e1b7de7da11a18b527aa2282b312952b692781dfd",
            },
            ModelFile {
                name: WEIGHTS_FILE,
                size: 2_271_064_456,
                sha256: "c18156e80caf8ff45eb84a24a853130c3bca03087ccb41b051f86e7556bae02c",
            },
            ModelFile {
                name: "1_Pooling/config.json",
                size: 297,
                sha256: "13e69897522ee8255104483ed9f219465d1be3936654a54a318758738052789e",
            },
            ModelFile {
                name: "modules.json",
                size: 349,
                sha256: "84e40c8e006c9b1d6c122e02cba9b02458120b5fb0c87b746c41e0207cf642cf",
            },
            ModelFile {
                name: "sentence_bert_config.json",
                size: 54,
                sha256: "eb9b44b13c0f52a3b3685c3b1cbdea1ba8b04bea123b98f61610048940776eb1",
            },
        ],
    },
];

pub fn model_manifest(id: &str) -> Option<&'static ModelManifest> {
    MODEL_MANIFESTS.iter().find(|manifest| manifest.id == id)
}

impl ModelManifest {
    /// `<org>--<name>@<revision>`: the folder the model's files live in.
    pub fn directory_name(&self) -> String {
        format!("{}@{}", self.id.replace('/', "--"), self.revision)
    }

    pub fn file(&self, name: &str) -> Option<&ModelFile> {
        self.files.iter().find(|file| file.name == name)
    }

    pub fn download_url(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.id, self.revision, file.name
        )
    }
}
