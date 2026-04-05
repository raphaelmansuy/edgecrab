# Creating Skills

Verified against `crates/edgecrab-tools/src/tools/skills.rs`.

The unit of reuse is a directory with a `SKILL.md` file. The runtime reads frontmatter when present, but the body remains plain Markdown.

## Minimal shape

```text
~/.edgecrab/skills/my-skill/
  -> SKILL.md
```

## Frontmatter fields the parser understands

- `name`
- `description`
- `category`
- `version`
- `license`
- `platforms`
- `read_files`
- `required_environment_variables`
- conditional activation fields for required or fallback tools and toolsets

## Good skill structure

Keep the body practical:

- when to use the skill
- prerequisites
- the shortest reliable workflow
- commands or file patterns that matter
- failure cases or fallback paths

## What the runtime does with it

- extracts display metadata for lists and summaries
- can load additional referenced files named in `read_files`
- can hide or surface skills based on tool and toolset availability
- can search recursively through local and external skill roots by directory name

## Operational note

You do not need YAML frontmatter for a skill to work. Frontmatter improves discovery and activation, but `SKILL.md` alone is enough.
