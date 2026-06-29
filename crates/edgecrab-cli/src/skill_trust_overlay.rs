//! Skill trust / guard approval overlay — key dispatch (Hermes ScanPanel parity).

use crossterm::event::{KeyCode, KeyModifiers};

pub const SKILL_TRUST_ACTION_LABELS: [&str; 3] = ["trust & install", "trust only", "cancel"];

pub const SKILL_CAUTION_ACTION_LABELS: [&str; 2] = ["force install", "cancel"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillTrustPane {
    #[default]
    Findings,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SkillTrustFilesFocus {
    #[default]
    List,
    Content,
}

pub const SKILL_REVIEW_TRUST_LABELS: [&str; 2] = ["record trust", "cancel"];
pub const SKILL_REVIEW_DISMISS_LABELS: [&str; 1] = ["dismiss"];

#[derive(Debug, Clone, Copy)]
pub struct SkillTrustKeyContext {
    pub pane: SkillTrustPane,
    pub needs_trust: bool,
    pub review_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTrustOverlayAction {
    SelectPrevAction,
    SelectNextAction,
    ScrollUp,
    ScrollDown,
    TogglePane,
    ToggleFilesFocus,
    JumpToFindingFile,
    Confirm,
    Cancel,
    Choose(usize),
    Noop,
}

pub fn skill_trust_action_count(needs_trust: bool, review_only: bool) -> usize {
    if review_only {
        if needs_trust { 2 } else { 1 }
    } else if needs_trust {
        3
    } else {
        2
    }
}

pub fn skill_trust_action_labels(needs_trust: bool, review_only: bool) -> &'static [&'static str] {
    if review_only {
        if needs_trust {
            &SKILL_REVIEW_TRUST_LABELS
        } else {
            &SKILL_REVIEW_DISMISS_LABELS
        }
    } else if needs_trust {
        &SKILL_TRUST_ACTION_LABELS
    } else {
        &SKILL_CAUTION_ACTION_LABELS
    }
}

pub fn map_skill_trust_key(
    code: KeyCode,
    modifiers: KeyModifiers,
    ctx: SkillTrustKeyContext,
) -> SkillTrustOverlayAction {
    if modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return SkillTrustOverlayAction::Noop;
    }

    let max_choice = skill_trust_action_count(ctx.needs_trust, ctx.review_only);

    match code {
        KeyCode::Esc => SkillTrustOverlayAction::Cancel,
        KeyCode::Tab | KeyCode::BackTab => SkillTrustOverlayAction::TogglePane,
        KeyCode::Enter => SkillTrustOverlayAction::Confirm,
        KeyCode::Char('f') if ctx.pane == SkillTrustPane::Findings => {
            SkillTrustOverlayAction::JumpToFindingFile
        }
        KeyCode::Char('v') if ctx.pane == SkillTrustPane::Findings => {
            SkillTrustOverlayAction::JumpToFindingFile
        }
        KeyCode::Left | KeyCode::Char('h') if ctx.pane == SkillTrustPane::Findings => {
            SkillTrustOverlayAction::SelectPrevAction
        }
        KeyCode::Right | KeyCode::Char('l') if ctx.pane == SkillTrustPane::Findings => {
            SkillTrustOverlayAction::SelectNextAction
        }
        KeyCode::Left | KeyCode::Char('h') if ctx.pane == SkillTrustPane::Files => {
            SkillTrustOverlayAction::ToggleFilesFocus
        }
        KeyCode::Right | KeyCode::Char('l') if ctx.pane == SkillTrustPane::Files => {
            SkillTrustOverlayAction::ToggleFilesFocus
        }
        KeyCode::Up | KeyCode::Char('k') => SkillTrustOverlayAction::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') => SkillTrustOverlayAction::ScrollDown,
        KeyCode::Char(c @ '1'..='3') if ctx.needs_trust && !ctx.review_only => {
            let idx = (c as u8 - b'1') as usize;
            if idx < max_choice {
                SkillTrustOverlayAction::Choose(idx)
            } else {
                SkillTrustOverlayAction::Noop
            }
        }
        KeyCode::Char('1') if !ctx.needs_trust || ctx.review_only => {
            SkillTrustOverlayAction::Choose(0)
        }
        KeyCode::Char('2') if !ctx.needs_trust && !ctx.review_only => {
            SkillTrustOverlayAction::Choose(1)
        }
        _ => SkillTrustOverlayAction::Noop,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esc_cancels() {
        assert_eq!(
            map_skill_trust_key(
                KeyCode::Esc,
                KeyModifiers::NONE,
                SkillTrustKeyContext {
                    needs_trust: true,
                    ..Default::default()
                }
            ),
            SkillTrustOverlayAction::Cancel
        );
    }

    #[test]
    fn dangerous_has_three_actions() {
        assert_eq!(skill_trust_action_count(true, false), 3);
        assert_eq!(skill_trust_action_count(true, true), 2);
        assert_eq!(
            map_skill_trust_key(
                KeyCode::Char('3'),
                KeyModifiers::NONE,
                SkillTrustKeyContext {
                    needs_trust: true,
                    ..Default::default()
                }
            ),
            SkillTrustOverlayAction::Choose(2)
        );
    }

    #[test]
    fn tab_switches_to_files_from_findings() {
        assert_eq!(
            map_skill_trust_key(
                KeyCode::Tab,
                KeyModifiers::NONE,
                SkillTrustKeyContext {
                    pane: SkillTrustPane::Findings,
                    needs_trust: false,
                    ..Default::default()
                }
            ),
            SkillTrustOverlayAction::TogglePane
        );
    }
}

impl Default for SkillTrustKeyContext {
    fn default() -> Self {
        Self {
            pane: SkillTrustPane::Findings,
            needs_trust: false,
            review_only: false,
        }
    }
}
