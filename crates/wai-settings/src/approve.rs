use std::collections::BTreeSet;

use crate::{BUILTIN_AGENTS, Settings};

#[derive(Debug, Clone, PartialEq)]
pub enum Approval {
    Agent {
        id: String,
        old: Option<String>,
        new: String,
    },
    Shell {
        old: Option<String>,
        new: String,
    },
}

pub fn seed_approved(settings: &Settings) -> BTreeSet<String> {
    settings
        .agents
        .values()
        .map(|a| a.command.clone())
        .chain(settings.terminal.shell.clone())
        .collect()
}

pub fn is_approved(value: &str, approved: &BTreeSet<String>) -> bool {
    approved.contains(value) || BUILTIN_AGENTS.iter().any(|a| a.command == value)
}

/// `old` is the value in force, `None` for an agent that is new.
pub fn unapproved(old: &Settings, new: &Settings, approved: &BTreeSet<String>) -> Vec<Approval> {
    let mut out: Vec<Approval> = new
        .agents
        .iter()
        .filter(|(_, a)| !is_approved(&a.command, approved))
        .map(|(id, a)| Approval::Agent {
            id: id.clone(),
            old: old.agents.get(id).map(|o| o.command.clone()),
            new: a.command.clone(),
        })
        .collect();
    if let Some(shell) = new
        .terminal
        .shell
        .as_ref()
        .filter(|s| !approved.contains(*s))
    {
        out.push(Approval::Shell {
            old: old.terminal.shell.clone(),
            new: shell.clone(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentConfig;

    #[test]
    fn builtins_are_approved_and_new_values_are_not() {
        let approved = BTreeSet::new();
        let old = Settings::default();
        assert!(unapproved(&old, &old, &approved).is_empty());
        let mut new = old.clone();
        new.agents.get_mut("codex").unwrap().command = "evil".into();
        new.agents.insert(
            "mine".into(),
            AgentConfig {
                command: "mine-acp".into(),
            },
        );
        new.terminal.shell = Some("/bin/evil".into());
        assert_eq!(
            unapproved(&old, &new, &approved),
            [
                Approval::Agent {
                    id: "codex".into(),
                    old: Some("npx -y @agentclientprotocol/codex-acp@2.6.0".into()),
                    new: "evil".into()
                },
                Approval::Agent {
                    id: "mine".into(),
                    old: None,
                    new: "mine-acp".into()
                },
                Approval::Shell {
                    old: None,
                    new: "/bin/evil".into()
                },
            ]
        );
        let seeded = seed_approved(&new);
        assert!(unapproved(&old, &new, &seeded).is_empty());
    }
}
