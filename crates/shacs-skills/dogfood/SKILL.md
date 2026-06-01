---
name: dogfood
description: "Exploratory QA of web apps: find bugs, evidence, reports."
version: 1.0.0
platforms: [linux, macos, windows]
metadata:
  hermes:
    tags: [qa, testing, browser, web, dogfood]
    related_skills: []
metadata.shacs.imported_from: dogfood
---

> shacs-bot adaptation: This skill was adapted for shacs-bot from the checked-in upstream reference. Use shacs-bot workspace and built-in skill paths, and prefer shacs-bot tools such as `exec`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `web_fetch`, `web_search`, `spawn`, and `ask_user`. Source-specific commands, services, or slash commands mentioned below are reference material unless the same capability is configured in shacs-bot.

# Dogfood: Systematic Web Application QA Testing

## Overview

This skill guides you through systematic exploratory QA testing of web applications using the browser toolset. You will navigate the application, interact with elements, capture evidence of issues, and produce a structured bug report.

## Prerequisites

- Browser toolset must be available (`configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`, `configured browser MCP tool`)
- A target URL and testing scope from the user

## Inputs

The user provides:
1. **Target URL** — the entry point for testing
2. **Scope** — what areas/features to focus on (or "full site" for comprehensive testing)
3. **Output directory** (optional) — where to save screenshots and the report (default: `./dogfood-output`)

## Workflow

Follow this 5-phase systematic workflow:

### Phase 1: Plan

1. Create the output directory structure:
   ```
   {output_dir}/
   ├── screenshots/       # Evidence screenshots
   └── report.md          # Final report (generated in Phase 5)
   ```
2. Identify the testing scope based on user input.
3. Build a rough sitemap by planning which pages and features to test:
   - Landing/home page
   - Navigation links (header, footer, sidebar)
   - Key user flows (sign up, login, search, checkout, etc.)
   - Forms and interactive elements
   - Edge cases (empty states, error pages, 404s)

### Phase 2: Explore

For each page or feature in your plan:

1. **Navigate** to the page:
   ```
   configured browser MCP tool(url="https://example.com/page")
   ```

2. **Take a snapshot** to understand the DOM structure:
   ```
   configured browser MCP tool()
   ```

3. **Check the console** for JavaScript errors:
   ```
   configured browser MCP tool(clear=true)
   ```
   Do this after every navigation and after every significant interaction. Silent JS errors are high-value findings.

4. **Take an annotated screenshot** to visually assess the page and identify interactive elements:
   ```
   configured browser MCP tool(question="Describe the page layout, identify any visual issues, broken elements, or accessibility concerns", annotate=true)
   ```
   The `annotate=true` flag overlays numbered `[N]` labels on interactive elements. Each `[N]` maps to ref `@eN` for subsequent browser commands.

5. **Test interactive elements** systematically:
   - Click buttons and links: `configured browser MCP tool(ref="@eN")`
   - Fill forms: `configured browser MCP tool(ref="@eN", text="test input")`
   - Test keyboard navigation: `configured browser MCP tool(key="Tab")`, `configured browser MCP tool(key="Enter")`
   - Scroll through content: `configured browser MCP tool(direction="down")`
   - Test form validation with invalid inputs
   - Test empty submissions

6. **After each interaction**, check for:
   - Console errors: `configured browser MCP tool()`
   - Visual changes: `configured browser MCP tool(question="What changed after the interaction?")`
   - Expected vs actual behavior

### Phase 3: Collect Evidence

For every issue found:

1. **Take a screenshot** showing the issue:
   ```
   configured browser MCP tool(question="Capture and describe the issue visible on this page", annotate=false)
   ```
   Save the `screenshot_path` from the response — you will reference it in the report.

2. **Record the details**:
   - URL where the issue occurs
   - Steps to reproduce
   - Expected behavior
   - Actual behavior
   - Console errors (if any)
   - Screenshot path

3. **Classify the issue** using the issue taxonomy (see `references/issue-taxonomy.md`):
   - Severity: Critical / High / Medium / Low
   - Category: Functional / Visual / Accessibility / Console / UX / Content

### Phase 4: Categorize

1. Review all collected issues.
2. De-duplicate — merge issues that are the same bug manifesting in different places.
3. Assign final severity and category to each issue.
4. Sort by severity (Critical first, then High, Medium, Low).
5. Count issues by severity and category for the executive summary.

### Phase 5: Report

Generate the final report using the template at `templates/dogfood-report-template.md`.

The report must include:
1. **Executive summary** with total issue count, breakdown by severity, and testing scope
2. **Per-issue sections** with:
   - Issue number and title
   - Severity and category badges
   - URL where observed
   - Description of the issue
   - Steps to reproduce
   - Expected vs actual behavior
   - Screenshot references (use `MEDIA:<screenshot_path>` for inline images)
   - Console errors if relevant
3. **Summary table** of all issues
4. **Testing notes** — what was tested, what was not, any blockers

Save the report to `{output_dir}/report.md`.

## Tools Reference

| Tool | Purpose |
|------|---------|
| `configured browser MCP tool` | Go to a URL |
| `configured browser MCP tool` | Get DOM text snapshot (accessibility tree) |
| `configured browser MCP tool` | Click an element by ref (`@eN`) or text |
| `configured browser MCP tool` | Type into an input field |
| `configured browser MCP tool` | Scroll up/down on the page |
| `configured browser MCP tool` | Go back in browser history |
| `configured browser MCP tool` | Press a keyboard key |
| `configured browser MCP tool` | Screenshot + AI analysis; use `annotate=true` for element labels |
| `configured browser MCP tool` | Get JS console output and errors |

## Tips

- **Always check `configured browser MCP tool()` after navigating and after significant interactions.** Silent JS errors are among the most valuable findings.
- **Use `annotate=true` with `configured browser MCP tool`** when you need to reason about interactive element positions or when the snapshot refs are unclear.
- **Test with both valid and invalid inputs** — form validation bugs are common.
- **Scroll through long pages** — content below the fold may have rendering issues.
- **Test navigation flows** — click through multi-step processes end-to-end.
- **Check responsive behavior** by noting any layout issues visible in screenshots.
- **Don't forget edge cases**: empty states, very long text, special characters, rapid clicking.
- When reporting screenshots to the user, include `MEDIA:<screenshot_path>` so they can see the evidence inline.
