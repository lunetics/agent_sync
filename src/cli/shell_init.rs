//! `agentsync shell-init`: `cmd_shell_init` of `lib/helpers/shell_init.sh`,
//! which prints the shell hook that runs `sync --if-stale` on entering a
//! project. Stdout carries nothing but the snippet, so `>> ~/.zshrc` stays clean.

use std::io::Write;

use super::customize::put;
use crate::Error;
use crate::log::Log;
use crate::style::Style;

const USAGE: &str = "Usage: agentsync shell-init [zsh|bash]

  Prints a shell snippet that runs 'agentsync sync --if-stale' for the
  current .ai/ project when you enter its root directory, so generated
  outputs stay fresh without syncing parent projects from descendants.

  Recommended: add one of these to your rc file. Eval'ing it regenerates
  the hook each session, so upgrades and fixes apply without re-editing:

    eval \"$(agentsync shell-init zsh)\"      # in ~/.zshrc
    eval \"$(agentsync shell-init bash)\"     # in ~/.bashrc

  Or freeze a copy with 'agentsync shell-init zsh >> ~/.zshrc', but then
  re-run it after each upgrade to pick up changes.

  The shell is auto-detected from $SHELL when omitted.
  Set AGENTSYNC_NO_AUTO_SYNC=1 to disable without removing the snippet.
";

const COMMON: &str = "_agentsync_autosync() {
  # Never `cd` here: as a zsh chpwd hook this fires on every directory change,
  # so a `cd` would re-trigger it and recurse (FUNCNEST blow-up). Point the sync
  # at the project via AGENTSYNC_REPO_ROOT instead, and guard against re-entry.
  [ -n \"${_AGENTSYNC_BUSY:-}\" ] && return 0
  [ -n \"${AGENTSYNC_NO_AUTO_SYNC:-}\" ] && return 0
  command -v agentsync >/dev/null 2>&1 || return 0
  _AGENTSYNC_BUSY=1
  if [ -d \"$PWD/.ai/src\" ]; then
    AGENTSYNC_REPO_ROOT=\"$PWD\" agentsync sync --if-stale || true
  fi
  unset _AGENTSYNC_BUSY
}
";

const ZSH: &str = "autoload -Uz add-zsh-hook 2>/dev/null
if (( ${+functions[add-zsh-hook]} )); then
  add-zsh-hook chpwd _agentsync_autosync
fi
_agentsync_autosync
# <<< agentsync shell hook (zsh) <<<
";

const BASH: &str = "_agentsync_prompt_hook() {
  if [ \"$PWD\" != \"${_AGENTSYNC_LAST_PWD:-}\" ]; then
    _AGENTSYNC_LAST_PWD=$PWD
    _agentsync_autosync
  fi
}
case \"${PROMPT_COMMAND:-}\" in
  *_agentsync_prompt_hook*) : ;;
  *) PROMPT_COMMAND=\"_agentsync_prompt_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}\" ;;
esac
_AGENTSYNC_LAST_PWD=$PWD
_agentsync_autosync
# <<< agentsync shell hook (bash) <<<
";

/// The snippet for `zsh` or `bash`.
pub fn snippet(shell: &str) -> String {
    let tail = if shell == "zsh" { ZSH } else { BASH };
    format!("# >>> agentsync shell hook ({shell}) >>>\n{COMMON}{tail}")
}

/// `cmd_shell_init`: `shell_env` is `$SHELL`, `colors` is `_use_colors` for
/// the log line; only the first argument is read, as Bash read it.
pub fn shell_init(
    args: &[String],
    shell_env: Option<&str>,
    style: &Style,
    colors: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut shell = args.first().cloned().unwrap_or_default();
    if shell == "--help" || shell == "-h" {
        return put(out, USAGE.as_bytes()).map(|()| 0);
    }
    if shell.is_empty() {
        match shell_env.unwrap_or("") {
            s if s.ends_with("/zsh") => shell = "zsh".to_string(),
            s if s.ends_with("/bash") => shell = "bash".to_string(),
            _ => {}
        }
    }
    let error = |message: &str| -> String {
        let mut log = Log::capturing(colors);
        log.error(message);
        log.lines()
            .iter()
            .map(|(_, line)| format!("{line}\n"))
            .collect()
    };
    match shell.as_str() {
        "zsh" | "bash" => put(out, snippet(&shell).as_bytes()).map(|()| 0),
        "" => {
            put(
                err,
                error("Could not detect your shell from $SHELL.").as_bytes(),
            )?;
            put(
                err,
                format!(
                    "  Pass one explicitly: {} or {}\n",
                    style.cyan("agentsync shell-init zsh"),
                    style.cyan("agentsync shell-init bash")
                )
                .as_bytes(),
            )?;
            Ok(2)
        }
        other => {
            put(
                err,
                error(&format!(
                    "Unsupported shell: {other} (expected 'zsh' or 'bash')"
                ))
                .as_bytes(),
            )?;
            Ok(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &[&str], shell: Option<&str>, colors: bool) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = shell_init(&args, shell, &Style::plain(), colors, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn the_snippets_carry_the_markers_and_the_hook_like_shell_init() {
        let (status, zsh, err) = run(&["zsh", "extra"], None, false);
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(zsh.starts_with("# >>> agentsync shell hook (zsh) >>>\n_agentsync_autosync() {\n"));
        assert!(zsh.contains("\n  add-zsh-hook chpwd _agentsync_autosync\n"));
        assert!(
            zsh.contains("\n    AGENTSYNC_REPO_ROOT=\"$PWD\" agentsync sync --if-stale || true\n")
        );
        assert!(zsh.ends_with("_agentsync_autosync\n# <<< agentsync shell hook (zsh) <<<\n"));
        assert!(!zsh.contains("cd "));
        let (_, bash, _) = run(&["bash"], None, false);
        assert!(bash.starts_with("# >>> agentsync shell hook (bash) >>>\n"));
        assert!(bash.contains(
            "PROMPT_COMMAND=\"_agentsync_prompt_hook${PROMPT_COMMAND:+;$PROMPT_COMMAND}\""
        ));
        assert!(bash.ends_with(
            "_AGENTSYNC_LAST_PWD=$PWD\n_agentsync_autosync\n# <<< agentsync shell hook (bash) <<<\n"
        ));
        assert_eq!(run(&[], Some("/usr/bin/zsh"), false).1, zsh);
        assert_eq!(run(&[], Some("/bin/bash"), false).1, bash);
        let (status, out, _) = run(&["--help"], None, false);
        assert_eq!((status, out.as_str()), (0, USAGE));
    }

    #[test]
    fn an_unknown_or_undetected_shell_exits_2_with_the_log_voice() {
        let (status, out, err) = run(&["fish"], None, false);
        assert_eq!((status, out.as_str()), (2, ""));
        assert_eq!(
            err,
            "[ERROR] Unsupported shell: fish (expected 'zsh' or 'bash')\n"
        );
        for shell in [None, Some(""), Some("zsh"), Some("/usr/local/bin/fish")] {
            let (status, out, err) = run(&[], shell, false);
            assert_eq!((status, out.as_str()), (2, ""));
            assert_eq!(
                err,
                "[ERROR] Could not detect your shell from $SHELL.\n  Pass one explicitly: agentsync shell-init zsh or agentsync shell-init bash\n"
            );
        }
        let (_, _, err) = run(&["fish"], None, true);
        assert_eq!(
            err,
            "\x1b[0;31m❌ [ERROR]\x1b[0m Unsupported shell: fish (expected 'zsh' or 'bash')\n"
        );
    }
}
