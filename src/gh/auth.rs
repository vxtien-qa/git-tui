use super::client;

/// Result of the startup auth check.
#[derive(Debug, Clone)]
pub struct AuthStatus {
    pub gh_installed: bool,
    pub gh_version: Option<String>,
    pub logged_in: bool,
    pub username: Option<String>,
    pub has_repo_scope: bool,
    pub has_project_scope: bool,
    pub has_read_org_scope: bool,
}

impl AuthStatus {
    /// Whether all checks passed.
    pub fn is_ready(&self) -> bool {
        self.gh_installed
            && self.logged_in
            && self.has_repo_scope
            && self.has_project_scope
            && self.has_read_org_scope
    }

    /// Missing scopes for display.
    pub fn missing_scopes(&self) -> Vec<&str> {
        let mut missing = Vec::new();
        if !self.has_repo_scope {
            missing.push("repo");
        }
        if !self.has_project_scope {
            missing.push("project");
        }
        if !self.has_read_org_scope {
            missing.push("read:org");
        }
        missing
    }
}

/// Run the full auth check sequence.
pub fn check() -> AuthStatus {
    // Step 1: Check gh binary
    let (gh_installed, gh_version) = match client::version() {
        Ok(v) => (true, Some(v)),
        Err(_) => {
            return AuthStatus {
                gh_installed: false,
                gh_version: None,
                logged_in: false,
                username: None,
                has_repo_scope: false,
                has_project_scope: false,
                has_read_org_scope: false,
            }
        }
    };

    // Step 2: Check auth status
    // IMPORTANT: gh auth status outputs to stdout on some platforms/versions
    // but to stderr on others. Always read BOTH to ensure we capture the output.
    let (logged_in, username, scopes_str) = {
        let output = std::process::Command::new("gh")
            .args(["auth", "status"])
            .output();
        match output {
            Ok(o) => {
                let combined = format!(
                    "{}\n{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                parse_auth_status(&combined)
            }
            Err(_) => (false, None, String::new()),
        }
    };

    // Step 3: Check scopes - parse the quoted list instead of substring
    // matching ("public_repo" must not satisfy "repo", "read:project" must
    // not satisfy the full "project" write scope).
    let scopes: Vec<String> = scopes_str
        .split(',')
        .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
        .collect();
    let has_scope = |name: &str| scopes.iter().any(|s| s == name);
    let has_repo_scope = has_scope("repo");
    let has_project_scope = has_scope("project");
    let has_read_org_scope = has_scope("read:org") || has_scope("admin:org");

    AuthStatus {
        gh_installed,
        gh_version,
        logged_in,
        username,
        has_repo_scope,
        has_project_scope,
        has_read_org_scope,
    }
}

/// Parse the output of `gh auth status` to extract login and scopes.
fn parse_auth_status(output: &str) -> (bool, Option<String>, String) {
    let logged_in = output.contains("Logged in to");
    let username = output
        .lines()
        .find(|l| l.contains("account"))
        .and_then(|l| {
            // "  ✓ Logged in to github.com account vxtien-qa (...)"
            l.split_whitespace()
                .skip_while(|w| *w != "account")
                .nth(1)
                .map(|s| {
                    s.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
                        .to_string()
                })
        });

    // Keep only the part after the colon: "- Token scopes: 'gist', 'repo'"
    let scopes = output
        .lines()
        .find(|l| l.contains("Token scopes"))
        .and_then(|l| l.split_once(':').map(|(_, rest)| rest.to_string()))
        .unwrap_or_default();

    (logged_in, username, scopes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_auth_status_fully_logged_in() {
        let output = "\
github.com
  ✓ Logged in to github.com account vxtien-qa (keyring)
  - Active account: true
  - Git operations protocol: https
  - Token: gho_************************************
  - Token scopes: 'gist', 'read:org', 'repo', 'workflow'";

        let (logged_in, username, scopes) = parse_auth_status(output);
        assert!(logged_in);
        assert_eq!(username.as_deref(), Some("vxtien-qa"));
        assert!(scopes.contains("repo"));
        assert!(scopes.contains("read:org"));
        assert!(!scopes.contains("project")); // This token is missing project
    }

    #[test]
    fn test_parse_auth_status_with_all_required_scopes() {
        let output = "\
github.com
  ✓ Logged in to github.com account testuser (keyring)
  - Token scopes: 'repo', 'project', 'read:org'";

        let (logged_in, username, scopes) = parse_auth_status(output);
        assert!(logged_in);
        assert_eq!(username.as_deref(), Some("testuser"));
        assert!(scopes.contains("repo"));
        assert!(scopes.contains("project"));
        assert!(scopes.contains("read:org"));
    }

    #[test]
    fn test_parse_auth_status_not_logged_in() {
        let output =
            "You are not logged in to any GitHub hosts. Run gh auth login to authenticate.";
        let (logged_in, username, scopes) = parse_auth_status(output);
        assert!(!logged_in);
        assert!(username.is_none());
        assert!(scopes.is_empty());
    }

    #[test]
    fn test_check_is_ready() {
        let ready = AuthStatus {
            gh_installed: true,
            gh_version: Some("2.87.2".into()),
            logged_in: true,
            username: Some("user".into()),
            has_repo_scope: true,
            has_project_scope: true,
            has_read_org_scope: true,
        };
        assert!(ready.is_ready());

        let missing_project = AuthStatus {
            has_project_scope: false,
            ..ready
        };
        assert!(!missing_project.is_ready());
        assert_eq!(missing_project.missing_scopes(), vec!["project"]);
    }
}
