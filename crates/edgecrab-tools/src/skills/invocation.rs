use super::bundles::{
    build_bundle_invocation_message, get_skill_bundles, resolve_bundle_command_key,
};
use super::context::SkillsScanContext;
use super::discovery::{resolve_skill_command_key, scan_skill_commands};
use super::invocation_extras::{format_slash_setup_note, format_slash_supporting_files};
use super::usage::bump_use;
use crate::tools::skills::load_skill_prompt_bundle;

const SKILL_DIR_HINT: &str = "Resolve relative paths in this skill (scripts/, templates/, references/, assets/) \
against that directory, then run them with the terminal tool using absolute paths.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlashInvocationKind {
    Bundle,
    Skill,
}

#[derive(Debug, Clone)]
pub struct SlashInvocation {
    pub kind: SlashInvocationKind,
    pub slug: String,
    pub message: String,
}

/// Build a Hermes-style user message for `/skill-slug [instruction]`.
pub fn build_skill_invocation_message(
    ctx: &SkillsScanContext,
    slug: &str,
    user_instruction: &str,
    session_id: Option<&str>,
) -> Option<String> {
    let commands = scan_skill_commands(ctx);
    let info = commands.get(slug)?;
    bump_use(&ctx.edgecrab_home, &info.name);
    let body = load_skill_prompt_bundle(
        &ctx.edgecrab_home,
        &ctx.external_skill_dirs,
        &info.name,
        session_id,
    )?;
    let mut parts = vec![
        format!(
            "[IMPORTANT: The user has invoked the \"{}\" skill, indicating they want \
             you to follow its instructions. The full skill content is loaded below.]",
            info.name
        ),
        body,
        format!(
            "[Skill directory: {}]\n{SKILL_DIR_HINT}",
            info.skill_dir.display()
        ),
    ];
    if let Some(note) = format_slash_setup_note(&info.skill_dir, ctx.interactive) {
        parts.push(note);
    }
    if let Some(supporting) = format_slash_supporting_files(&info.skill_dir, &info.name) {
        parts.push(supporting);
    }
    if !user_instruction.trim().is_empty() {
        parts.push(format!(
            "The user provided the following instruction alongside the skill invocation: {}",
            user_instruction.trim()
        ));
    }
    Some(parts.join("\n\n"))
}

/// Resolve `/token [rest]` — bundles win over skills (Hermes order).
pub fn resolve_slash_invocation(
    ctx: &SkillsScanContext,
    command_token: &str,
    user_instruction: &str,
    session_id: Option<&str>,
) -> Option<SlashInvocation> {
    let bundles = get_skill_bundles(ctx);
    if let Some(slug) = resolve_bundle_command_key(&bundles, command_token)
        && let Some(message) =
            build_bundle_invocation_message(ctx, &slug, user_instruction, session_id)
    {
        return Some(SlashInvocation {
            kind: SlashInvocationKind::Bundle,
            slug,
            message,
        });
    }
    let commands = scan_skill_commands(ctx);
    if let Some(slug) = resolve_skill_command_key(&commands, command_token)
        && let Some(message) =
            build_skill_invocation_message(ctx, &slug, user_instruction, session_id)
    {
        return Some(SlashInvocation {
            kind: SlashInvocationKind::Skill,
            slug,
            message,
        });
    }
    None
}

/// Parse a full slash line `/name optional instruction`.
pub fn parse_slash_line(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let rest = trimmed.trim_start_matches('/').trim_start();
    if rest.is_empty() {
        return None;
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let cmd = parts.next()?.trim();
    let instruction = parts.next().unwrap_or("").trim();
    if cmd.is_empty() {
        return None;
    }
    Some((cmd, instruction))
}

pub fn resolve_slash_line(
    ctx: &SkillsScanContext,
    input: &str,
    session_id: Option<&str>,
) -> Option<SlashInvocation> {
    let (cmd, instruction) = parse_slash_line(input)?;
    resolve_slash_invocation(ctx, cmd, instruction, session_id)
}

/// If `text` is a skill/bundle slash line, return the enriched agent message.
pub fn enrich_message_for_skill_slash(
    ctx: &SkillsScanContext,
    text: &str,
    session_id: Option<&str>,
) -> String {
    resolve_slash_line(ctx, text, session_id)
        .map(|inv| inv.message)
        .unwrap_or_else(|| text.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write_skill(home: &std::path::Path, slug: &str, name: &str) {
        let skill_dir = home.join("skills").join(slug);
        std::fs::create_dir_all(&skill_dir).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: test\n---\n\n# Body"),
        )
        .expect("write");
    }

    #[test]
    fn bundle_wins_over_skill_name_collision() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "research", "Research Skill");
        let bundles_dir = dir.path().join("skill-bundles");
        std::fs::create_dir_all(&bundles_dir).expect("mkdir bundles");
        std::fs::write(
            bundles_dir.join("research.yaml"),
            "name: research\nskills:\n  - research\n",
        )
        .expect("write bundle");
        let ctx = SkillsScanContext::from_home(dir.path());
        let inv = resolve_slash_invocation(&ctx, "research", "", None).expect("invocation");
        assert_eq!(inv.kind, SlashInvocationKind::Bundle);
    }

    #[test]
    fn enrich_slash_loads_skill_content() {
        let dir = TempDir::new().expect("tmpdir");
        write_skill(dir.path(), "demo", "demo");
        let ctx = SkillsScanContext::from_home(dir.path());
        let out = enrich_message_for_skill_slash(&ctx, "/demo focus on tests", None);
        assert!(out.contains("invoked"), "out={out}");
        assert!(out.contains("# Body"), "out={out}");
        assert!(out.contains("focus on tests"), "out={out}");
    }

    #[test]
    fn enrich_slash_includes_supporting_scripts() {
        let dir = TempDir::new().expect("tmpdir");
        let skill_dir = dir.path().join("skills").join("demo");
        let scripts = skill_dir.join("scripts");
        std::fs::create_dir_all(&scripts).expect("mkdir");
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: test\n---\n\n# Body",
        )
        .expect("write skill");
        std::fs::write(scripts.join("run.js"), "console.log('hi')").expect("write script");
        let ctx = SkillsScanContext::from_home(dir.path());
        let out = enrich_message_for_skill_slash(&ctx, "/demo", None);
        assert!(out.contains("scripts/run.js"), "out={out}");
        assert!(out.contains("skill_view"), "out={out}");
    }
}
