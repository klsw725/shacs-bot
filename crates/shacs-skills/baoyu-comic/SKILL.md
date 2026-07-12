---
name: baoyu-comic
description: "Knowledge comics (知识漫画): educational, biography, tutorial."
version: 1.56.1
author: 宝玉 (JimLiu)
license: MIT
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [comic, knowledge-comic, creative, image-generation]
    homepage: https://github.com/JimLiu/baoyu-skills#baoyu-comic
metadata.shacs.imported_from: creative/baoyu-comic
---

> shacs-bot adaptation: This skill was adapted for shacs-bot from the checked-in upstream reference. Use shacs-bot workspace and built-in skill paths, and prefer shacs-bot tools such as `exec`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `web_fetch`, `web_search`, `spawn`, and `ask_user`. Source-specific commands, services, or slash commands mentioned below are reference material unless the same capability is configured in shacs-bot.

# Knowledge Comic Creator

Adapted from [baoyu-comic](https://github.com/JimLiu/baoyu-skills) for shacs-bot's tool ecosystem.

Create original knowledge comics with flexible art style × tone combinations.

## When to Use

Trigger this skill when the user asks to create a knowledge/educational comic, biography comic, tutorial comic, or uses terms like "知识漫画", "教育漫画", or "Logicomix-style". The user provides content (text, file path, URL, or topic) and optionally specifies art style, tone, layout, aspect ratio, or language.

## Reference Images

shacs-bot's `image_generate` tool is **prompt-only**. It accepts `prompt` plus optional `size`, `quality`, `format`, `background`, and `count`, and returns local media artifact JSON. It does **NOT** accept reference images or an `aspect_ratio` parameter. When the user supplies a reference image, use it to **extract traits in text** that get embedded in every page prompt:

**Intake**: Accept file paths when the user provides them (or pastes images in conversation).
- File path(s) → copy to `refs/NN-ref-{slug}.{ext}` alongside the comic output for provenance
- Pasted image with no path → ask the user for the path via `ask_user`, or extract style traits verbally as a text fallback
- No reference → skip this section

**Usage modes** (per reference):

| Usage | Effect |
|-------|--------|
| `style` | Extract style traits (line treatment, texture, mood) and append to every page's prompt body |
| `palette` | Extract hex colors and append to every page's prompt body |
| `scene` | Extract scene composition or subject notes and append to the relevant page(s) |

**Record in each page's prompt frontmatter** when refs exist:

```yaml
references:
  - ref_id: 01
    filename: 01-ref-scene.png
    usage: style
    traits: "muted earth tones, soft-edged ink wash, low-contrast backgrounds"
```

Character consistency is driven by **text descriptions** in `characters/characters.md` (written in Step 3) that get embedded inline in every page prompt (Step 5). The optional PNG character sheet generated in Step 7.1 is a human-facing review artifact, not an input to `image_generate`.

## Options

### Visual Dimensions

| Option | Values | Description |
|--------|--------|-------------|
| Art | ligne-claire (default), manga, realistic, ink-brush, chalk, minimalist | Art style / rendering technique |
| Tone | neutral (default), warm, dramatic, romantic, energetic, vintage, action | Mood / atmosphere |
| Layout | standard (default), cinematic, dense, splash, mixed, webtoon, four-panel | Panel arrangement |
| Aspect | 3:4 (default, portrait), 4:3 (landscape), 16:9 (widescreen) | Page aspect ratio |
| Language | auto (default), zh, en, ja, etc. | Output language |
| Refs | File paths | Reference images used for style / palette trait extraction (not passed to the image model). See [Reference Images](#reference-images) above. |

### Partial Workflow Options

| Option | Description |
|--------|-------------|
| Storyboard only | Generate storyboard only, skip prompts and images |
| Prompts only | Generate storyboard + prompts, skip images |
| Images only | Generate images from existing prompts directory |
| Regenerate N | Regenerate specific page(s) only (e.g., `3` or `2,5,8`) |

Details: [references/partial-workflows.md](references/partial-workflows.md)

### Art, Tone & Preset Catalogue

- **Art styles** (6): `ligne-claire`, `manga`, `realistic`, `ink-brush`, `chalk`, `minimalist`. Full definitions at `references/art-styles/<style>.md`.
- **Tones** (7): `neutral`, `warm`, `dramatic`, `romantic`, `energetic`, `vintage`, `action`. Full definitions at `references/tones/<tone>.md`.
- **Presets** (5) with special rules beyond plain art+tone:

  | Preset | Equivalent | Hook |
  |--------|-----------|------|
  | `ohmsha` | manga + neutral | Visual metaphors, no talking heads, gadget reveals |
  | `wuxia` | ink-brush + action | Qi effects, combat visuals, atmospheric |
  | `shoujo` | manga + romantic | Decorative elements, eye details, romantic beats |
  | `concept-story` | manga + warm | Visual symbol system, growth arc, dialogue+action balance |
  | `four-panel` | minimalist + neutral + four-panel layout | 起承转合 structure, B&W + spot color, stick-figure characters |

  Full rules at `references/presets/<preset>.md` — load the file when a preset is picked.

- **Compatibility matrix** and **content-signal → preset** table live in [references/auto-selection.md](references/auto-selection.md). Read it before recommending combinations in Step 2.

## File Structure

Output directory: `comic/{topic-slug}/`
- Slug: 2-4 words kebab-case from topic (e.g., `alan-turing-bio`)
- Conflict: append timestamp (e.g., `turing-story-20260118-143052`)

**Contents**:
| File | Description |
|------|-------------|
| `source-{slug}.md` | Saved source content (kebab-case slug matches the output directory) |
| `analysis.md` | Content analysis |
| `storyboard.md` | Storyboard with panel breakdown |
| `characters/characters.md` | Character definitions |
| `characters/characters.png` | Character reference sheet copied from the `image_generate` artifact path |
| `prompts/NN-{cover\|page}-[slug].md` | Generation prompts |
| `NN-{cover\|page}-[slug].png` | Generated images copied from returned `image_generate` artifact paths |
| `refs/NN-ref-{slug}.{ext}` | User-supplied reference images (optional, for provenance) |

## Language Handling

**Detection Priority**:
1. User-specified language (explicit option)
2. User's conversation language
3. Source content language

**Rule**: Use user's input language for ALL interactions:
- Storyboard outlines and scene descriptions
- Image generation prompts
- User selection options and confirmations
- Progress updates, questions, errors, summaries

Technical terms remain in English.

## Workflow

### Progress Checklist

```
Comic Progress:
- [ ] Step 1: Setup & Analyze
  - [ ] 1.1 Analyze content
  - [ ] 1.2 Check existing directory
- [ ] Step 2: Confirmation - Style & options ⚠️ REQUIRED
- [ ] Step 3: Generate storyboard + characters
- [ ] Step 4: Review outline (conditional)
- [ ] Step 5: Generate prompts
- [ ] Step 6: Review prompts (conditional)
- [ ] Step 7: Generate images
  - [ ] 7.1 Generate character sheet (if needed) → characters/characters.png
  - [ ] 7.2 Generate pages (with character descriptions embedded in prompt)
- [ ] Step 8: Completion report
```

### Flow

```
Input → Analyze → [Check Existing?] → [Confirm: Style + Reviews] → Storyboard → [Review?] → Prompts → [Review?] → Images → Complete
```

### Step Summary

| Step | Action | Key Output |
|------|--------|------------|
| 1.1 | Analyze content | `analysis.md`, `source-{slug}.md` |
| 1.2 | Check existing directory | Handle conflicts |
| 2 | Confirm style, focus, audience, reviews | User preferences |
| 3 | Generate storyboard + characters | `storyboard.md`, `characters/` |
| 4 | Review outline (if requested) | User approval |
| 5 | Generate prompts | `prompts/*.md` |
| 6 | Review prompts (if requested) | User approval |
| 7.1 | Generate character sheet (if needed) | `characters/characters.png` |
| 7.2 | Generate pages | `*.png` files |
| 8 | Completion report | Summary |

### User Questions

Use the `ask_user` tool to confirm options. Since `ask_user` handles one question at a time, ask the most important question first and proceed sequentially. See [references/workflow.md](references/workflow.md) for the full Step 2 question set.

**Timeout handling (CRITICAL)**: `ask_user` can return `"The user did not provide a response within the time limit. Use your best judgement to make the choice and proceed."` — this is NOT user consent to default everything.

- Treat it as a default **for that one question only**. Continue asking the remaining Step 2 questions in sequence; each question is an independent consent point.
- **Surface the default to the user visibly** in your next message so they have a chance to correct it: e.g. `"Style: defaulted to ohmsha preset (clarify timed out). Say the word to switch."` — an unreported default is indistinguishable from never having asked.
- Do NOT collapse Step 2 into a single "use all defaults" pass after one timeout. If the user is genuinely absent, they will be equally absent for all five questions — but they can correct visible defaults when they return, and cannot correct invisible ones.

### Step 7: Image Generation

Use shacs-bot's built-in `image_generate` tool for all image rendering. Its schema accepts `prompt` plus optional `size`, `quality`, `format`, `background`, and `count`. It is prompt-only, so it does not accept reference image input or an `aspect_ratio` parameter. The result is local media artifact JSON with an `artifacts[]` array. Each artifact includes fields such as `mediaRef`, `path`, and `metadataRef`.

**Prompt file requirement (hard)**: write each image's full, final prompt to a standalone file under `prompts/` (naming: `NN-{type}-[slug].md`) BEFORE calling `image_generate`. The prompt file is the reproducibility record.

**Size selection**: preserve the storyboard's visual intent when choosing the optional `size` value. Use provider-supported size strings only. If no exact provider-supported size is known, leave `size` unset and keep the desired composition in the prompt text.

**Artifact step** after every `image_generate` call:
1. Read the first relevant entry from `artifacts[]`.
2. Use the returned `path` as the local source file, or keep `mediaRef` as the durable runtime reference.
3. If the workflow needs the image at a comic-specific filename, copy from `path` to the target output path and verify the target file exists and is non-empty. Do not move or rename the runtime artifact because `mediaRef` and `metadataRef` refer to it.
4. Do not curl a returned URL. The tool does not return a remote image URL.

**7.1 Character sheet**: generate it when the comic is multi-page with recurring characters. Skip for simple presets (e.g., four-panel minimalist) or single-page comics. The prompt file at `characters/characters.md` must exist before invoking `image_generate`. The rendered PNG is a **human-facing review artifact** (so the user can visually verify character design) and a reference for later regenerations or manual prompt edits. It does **not** drive Step 7.2. Page prompts are already written in Step 5 from the **text descriptions** in `characters/characters.md`; `image_generate` cannot accept images as visual input.

**7.2 Pages** — each page's prompt MUST already be at `prompts/NN-{cover|page}-[slug].md` before invoking `image_generate`. Because `image_generate` is prompt-only, character consistency is enforced by **embedding character descriptions (sourced from `characters/characters.md`) inline in every page prompt during Step 5**. The embedding is done uniformly whether or not a PNG sheet is produced in 7.1; the PNG is only a review/regeneration aid.

**Backup rule**: existing `prompts/…md` and `…png` files → rename with `-backup-YYYYMMDD-HHMMSS` suffix before regenerating.

Full step-by-step workflow (analysis, storyboard, review gates, regeneration variants): [references/workflow.md](references/workflow.md).

## References

**Core Templates**:
- [analysis-framework.md](references/analysis-framework.md) - Deep content analysis
- [character-template.md](references/character-template.md) - Character definition format
- [storyboard-template.md](references/storyboard-template.md) - Storyboard structure
- [ohmsha-guide.md](references/ohmsha-guide.md) - Ohmsha manga specifics

**Style Definitions**:
- `references/art-styles/` - Art styles (ligne-claire, manga, realistic, ink-brush, chalk, minimalist)
- `references/tones/` - Tones (neutral, warm, dramatic, romantic, energetic, vintage, action)
- `references/presets/` - Presets with special rules (ohmsha, wuxia, shoujo, concept-story, four-panel)
- `references/layouts/` - Layouts (standard, cinematic, dense, splash, mixed, webtoon, four-panel)

**Workflow**:
- [workflow.md](references/workflow.md) - Full workflow details
- [auto-selection.md](references/auto-selection.md) - Content signal analysis
- [partial-workflows.md](references/partial-workflows.md) - Partial workflow options

## Page Modification

| Action | Steps |
|--------|-------|
| **Edit** | **Update prompt file FIRST** → regenerate image → use returned artifact path for the new PNG |
| **Add** | Create prompt at position → generate with character descriptions embedded → renumber subsequent → update storyboard |
| **Delete** | Remove files → renumber subsequent → update storyboard |

**IMPORTANT**: When updating pages, ALWAYS update the prompt file (`prompts/NN-{cover|page}-[slug].md`) FIRST before regenerating. This ensures changes are documented and reproducible.

## Pitfalls

- Image generation: 10-30 seconds per page; auto-retry once on failure
- **Always use** the `path` or `mediaRef` returned by `image_generate`. Downstream tooling and the user's review expect local files or durable media references, not remote URLs.
- Use stylized alternatives for sensitive public figures
- **Step 2 confirmation required** - do not skip
- **Steps 4/6 conditional** - only if user requested in Step 2
- **Step 7.1 character sheet** - recommended for multi-page comics, optional for simple presets. The PNG is a review/regeneration aid; page prompts (written in Step 5) use the text descriptions in `characters/characters.md`, not the PNG. `image_generate` does not accept images as visual input
- **Strip secrets** — scan source content for API keys, tokens, or credentials before writing any output file
