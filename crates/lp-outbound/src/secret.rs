//! The `SecretResolver` port, and the passfile. Resolved here so a plaintext
//! password never enters `lp-domain`: an [`Effect`] carries only the
//! [`Credential`].
//!
//! [`Effect`]: lp_domain::update::Effect

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use lp_domain::config::Credential;
use lp_domain::ports::{ConfigError, SecretResolver};

#[derive(Debug, Clone, Copy, Default)]
pub struct Secrets;

/// `~user` and other paths are left alone.
pub(crate) fn expand_home(path: &Path) -> PathBuf {
    let Ok(rest) = path.strip_prefix("~") else {
        return path.to_owned();
    };

    std::env::var_os("HOME").map_or_else(|| path.to_owned(), |home| PathBuf::from(home).join(rest))
}

/// What a passfile line is matched against, as libpq resolves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Target<'a> {
    pub(crate) host: &'a str,
    pub(crate) port: u16,
    pub(crate) database: &'a str,
    pub(crate) user: &'a str,
}

/// `PGPASSFILE`, else `~/.pgpass`. `None` when unreadable, or readable by
/// group or others, which libpq also refuses.
pub(crate) async fn passfile(target: Target<'_>) -> Option<String> {
    let path = std::env::var_os("PGPASSFILE")
        .map_or_else(|| expand_home(Path::new("~/.pgpass")), PathBuf::from);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = tokio::fs::metadata(&path).await.ok()?.permissions().mode();
        if mode & 0o077 != 0 {
            return None;
        }
    }

    let text = tokio::fs::read_to_string(&path).await.ok()?;
    lookup(&text, target)
}

/// `hostname:port:database:username:password`, first match wins, `*`
/// matches anything, `\` escapes `:` and itself. `localhost` also matches a
/// Unix-socket host, as libpq.
fn lookup(text: &str, target: Target<'_>) -> Option<String> {
    let port = target.port.to_string();

    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let fields = fields(line);
            let [host, port_field, database, user, password] = fields.as_slice() else {
                return None;
            };
            let matches = |field: &str, value: &str| field == "*" || field == value;
            let host_matches =
                matches(host, target.host) || (host == "localhost" && target.host.starts_with('/'));

            (host_matches
                && matches(port_field, &port)
                && matches(database, target.database)
                && matches(user, target.user))
            .then(|| password.clone())
        })
}

fn fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let (Some(next), Some(field)) = (chars.next(), fields.last_mut()) {
                    field.push(next);
                }
            }
            ':' if fields.len() < 5 => fields.push(String::new()),
            c => {
                if let Some(field) = fields.last_mut() {
                    field.push(c);
                }
            }
        }
    }
    fields
}

#[async_trait]
impl SecretResolver for Secrets {
    async fn resolve(&self, credential: &Credential) -> Result<String, ConfigError> {
        match credential {
            Credential::Literal(value) => Ok(value.clone()),
            Credential::Env(name) => std::env::var(name).map_err(|_| {
                ConfigError::Unresolvable(format!("${name} is not set in the environment"))
            }),
            Credential::File(path) => {
                let path = expand_home(path);
                let value = tokio::fs::read_to_string(&path).await.map_err(|error| {
                    ConfigError::Unresolvable(format!("{}: {error}", path.display()))
                })?;

                // Only the newline: a password may begin or end with a space.
                Ok(value
                    .strip_suffix('\n')
                    .map_or(value.as_str(), |line| {
                        line.strip_suffix('\r').unwrap_or(line)
                    })
                    .to_owned())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: Target<'static> = Target {
        host: "db.internal",
        port: 5432,
        database: "orders",
        user: "ana",
    };

    #[test]
    fn the_first_matching_passfile_line_wins_and_stars_match_anything() {
        let text = "\
# a comment
other:5432:orders:ana:wrong-host
db.internal:*:orders:ana:first
*:*:*:*:fallback
";
        assert_eq!(lookup(text, TARGET).as_deref(), Some("first"));
        assert_eq!(
            lookup(
                text,
                Target {
                    user: "bob",
                    ..TARGET
                }
            )
            .as_deref(),
            Some("fallback")
        );
    }

    #[test]
    fn a_passfile_field_may_escape_its_colons() {
        let text = "db.internal:5432:orders:ana:pa\\:ss\\\\word\n";
        assert_eq!(lookup(text, TARGET).as_deref(), Some("pa:ss\\word"));
    }

    #[test]
    fn localhost_in_a_passfile_also_means_the_unix_socket() {
        let text = "localhost:5432:orders:ana:socket\n";
        assert_eq!(
            lookup(
                text,
                Target {
                    host: "/var/run/postgresql",
                    ..TARGET
                }
            )
            .as_deref(),
            Some("socket")
        );
    }

    #[test]
    fn a_short_passfile_line_matches_nothing() {
        assert_eq!(lookup("db.internal:5432:orders:ana\n", TARGET), None);
    }

    #[test]
    fn a_path_without_a_tilde_is_left_alone() {
        assert_eq!(
            expand_home(Path::new("/etc/lazypg/pw")),
            PathBuf::from("/etc/lazypg/pw")
        );
    }

    #[test]
    fn a_tilde_naming_another_user_is_not_guessed_at() {
        assert_eq!(
            expand_home(Path::new("~someone/.pgpass")),
            PathBuf::from("~someone/.pgpass"),
            "only a bare `~` is $HOME; a shell would resolve the rest and this is not a shell"
        );
    }

    #[tokio::test]
    async fn a_literal_resolves_to_itself() {
        let resolved = Secrets
            .resolve(&Credential::Literal("hunter2".to_owned()))
            .await;

        assert_eq!(resolved, Ok("hunter2".to_owned()));
    }

    #[tokio::test]
    async fn an_unset_variable_names_itself_rather_than_resolving_to_empty() {
        let resolved = Secrets
            .resolve(&Credential::Env(
                "LP_A_VARIABLE_NO_ENVIRONMENT_SETS".to_owned(),
            ))
            .await;

        assert!(matches!(resolved, Err(ConfigError::Unresolvable(_))));
    }
}
