//! The merged, validated configuration. `Option` stops at `lp-outbound`'s wire
//! structs, so nothing here asks whether a setting was stated.

use std::collections::BTreeMap;
use std::num::NonZeroU16;
use std::path::PathBuf;
use std::sync::Arc;

use crate::posture::Posture;
use crate::style::Severity;
use crate::theme::{Role, ThemeChoice, ThemeName};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{value} is not a valid {what}: {expected}")]
pub struct OutOfRange {
    pub what: &'static str,
    pub value: String,
    pub expected: &'static str,
}

macro_rules! bounded {
    ($name:ident, $inner:ty, $what:literal, $lo:expr, $hi:expr, $default:expr) => {
        #[doc = concat!("A validated `", $what, "`, between ", stringify!($lo), " and ", stringify!($hi), ".")]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name($inner);

        impl $name {
            /// # Errors
            ///
            /// When `value` falls outside the range.
            pub fn try_new(value: $inner) -> Result<Self, OutOfRange> {
                if ($lo..=$hi).contains(&value) {
                    Ok(Self(value))
                } else {
                    Err(OutOfRange {
                        what: $what,
                        value: value.to_string(),
                        expected: concat!(stringify!($lo), "..=", stringify!($hi)),
                    })
                }
            }

            #[must_use]
            pub const fn get(self) -> $inner {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self($default)
            }
        }
    };
}

// 30 holds a millisecond timestamp and stops short of a uuid's 36.
bounded!(MaxColumnWidth, u16, "max_column_width", 1, 512, 30);
bounded!(PageRows, u32, "page_rows", 1, 100_000, 200);
bounded!(PageSize, u32, "page_size", 1, 100_000, 5_000);
bounded!(HistoryLimit, u32, "limit", 0, 10_000, 100);

impl HistoryLimit {
    /// Zero keeps nothing.
    #[must_use]
    pub const fn is_off(self) -> bool {
        self.0 == 0
    }
}
bounded!(Millis, u32, "milliseconds", 1, 600_000, 5_000);
bounded!(ClipboardMb, u32, "clipboard_mb", 1, 1_024, 10);

impl ClipboardMb {
    /// 1024-based, as every size the app draws.
    #[must_use]
    pub const fn bytes(self) -> usize {
        self.0 as usize * 1024 * 1024
    }
}

/// The two share one range.
impl From<PageRows> for PageSize {
    fn from(rows: PageRows) -> Self {
        Self(rows.0)
    }
}

/// One type: a partial set of shares is meaningless. Each share is checked
/// against `5..=95` alone, not jointly against `layout.rs`'s floors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelRatios {
    sidebar: u16,
    ddl: u16,
    queries: u16,
}

impl PanelRatios {
    /// # Errors
    ///
    /// When any share falls outside 5..=95.
    pub fn try_new(sidebar: u16, ddl: u16, queries: u16) -> Result<Self, OutOfRange> {
        for (what, value) in [
            ("layout.sidebar", sidebar),
            ("layout.ddl", ddl),
            ("layout.queries", queries),
        ] {
            if !(5..=95).contains(&value) {
                return Err(OutOfRange {
                    what: "panel share",
                    value: format!("{what} = {value}"),
                    expected: "5..=95",
                });
            }
        }

        Ok(Self {
            sidebar,
            ddl,
            queries,
        })
    }

    #[must_use]
    pub const fn sidebar(self) -> u16 {
        self.sidebar
    }

    #[must_use]
    pub const fn ddl(self) -> u16 {
        self.ddl
    }

    #[must_use]
    pub const fn queries(self) -> u16 {
        self.queries
    }
}

impl Default for PanelRatios {
    fn default() -> Self {
        Self {
            sidebar: 25,
            ddl: 60,
            queries: 10,
        }
    }
}

/// Also the key the merge unions on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ConnectionName(Arc<str>);

impl ConnectionName {
    /// # Errors
    ///
    /// When `name` is blank or holds whitespace.
    pub fn try_new(name: &str) -> Result<Self, OutOfRange> {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err(OutOfRange {
                what: "connection name",
                value: name.to_owned(),
                expected: "non-empty, no whitespace",
            });
        }

        Ok(Self(Arc::from(name)))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Not `ratatui::Color`: `lp-outbound` never sees a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

impl Rgb {
    /// Parses `#rrggbb`.
    ///
    /// # Errors
    ///
    /// When the string is not `#` and six hex digits.
    pub fn parse(hex: &str) -> Result<Self, OutOfRange> {
        let out_of_range = || OutOfRange {
            what: "colour",
            value: hex.to_owned(),
            expected: "#rrggbb",
        };
        let digits = hex.strip_prefix('#').ok_or_else(out_of_range)?;

        if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(out_of_range());
        }

        let byte = |range: std::ops::Range<usize>| {
            digits
                .get(range)
                .and_then(|part| u8::from_str_radix(part, 16).ok())
                .ok_or_else(out_of_range)
        };

        Ok(Self {
            red: byte(0..2)?,
            green: byte(2..4)?,
            blue: byte(4..6)?,
        })
    }
}

/// `Terminal` paints nothing, which is what lets a translucent terminal show
/// through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    #[default]
    Theme,
    Terminal,
    Fixed(Rgb),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Icons {
    #[default]
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mouse {
    #[default]
    On,
    Off,
}

/// No `Command` source: a committed project file would make cloning and running
/// the tool execute a stranger's command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Credential {
    Literal(String),
    Env(String),
    File(PathBuf),
}

/// libpq's `sslmode`, less `allow`. `Prefer` and `Require` encrypt without
/// authenticating the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyCa,
    VerifyFull,
}

impl SslMode {
    pub const ALL: [Self; 5] = [
        Self::Disable,
        Self::Prefer,
        Self::Require,
        Self::VerifyCa,
        Self::VerifyFull,
    ];

    /// The one spelling a config file uses, libpq's.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyCa => "verify-ca",
            Self::VerifyFull => "verify-full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Trust {
    #[default]
    System,
    /// Held as written; `~` is expanded where it is read.
    CaFile(PathBuf),
}

/// Both paths or neither: a certificate without its key proves nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// An unstated field falls back as libpq's does — its `PG*` variable, then
/// libpq's default — resolved in `lp-outbound` at connect time, so the
/// environment of the moment applies. Absent `database` is the user's name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub name: ConnectionName,
    pub host: Option<String>,
    pub port: Option<NonZeroU16>,
    pub database: Option<String>,
    pub user: Option<String>,
    /// Unstated: `PGPASSWORD`, then the passfile.
    pub password: Option<Credential>,
    pub posture: Posture,
    pub sslmode: Option<SslMode>,
    pub trust: Trust,
    pub identity: Option<ClientIdentity>,
}

impl ConnectionConfig {
    /// Every field libpq defaults is unstated: the connection a bare
    /// `psql` would make.
    #[must_use]
    pub fn named(name: ConnectionName) -> Self {
        Self {
            name,
            host: None,
            port: None,
            database: None,
            user: None,
            password: None,
            posture: Posture::default(),
            sslmode: None,
            trust: Trust::System,
            identity: None,
        }
    }

    /// `host:port/database`, with what was not stated left out.
    #[must_use]
    pub fn address(&self) -> String {
        let mut address = self.host.clone().unwrap_or_else(|| "localhost".to_owned());
        if let Some(port) = self.port {
            address = format!("{address}:{port}");
        }
        if let Some(database) = &self.database {
            address = format!("{address}/{database}");
        }
        address
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GridConfig {
    pub max_column_width: MaxColumnWidth,
    pub page_rows: PageRows,
}

/// Whether the Logs tab lists the queries the app itself sends to read the
/// catalog. Hidden, they are one "Reading schema" entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CatalogQueries {
    #[default]
    Hidden,
    Shown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogConfig {
    pub catalog: CatalogQueries,
}

/// Zero disables the history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistoryConfig {
    pub limit: HistoryLimit,
}

/// Global: nothing yet calls for per-connection paging or timeouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DriverConfig {
    pub page_size: PageSize,
    pub request_timeout: Millis,
    pub connect_timeout: Millis,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExportConfig {
    /// Parsed but not consumed: the export form prefills from the working
    /// directory.
    pub path: Option<PathBuf>,
    pub prefix: Option<String>,
    /// A copy holds the whole result in memory; a file export streams.
    pub clipboard_mb: ClipboardMb,
}

/// `extends` is a [`BuiltIn`], so theme chains and their cycles are
/// unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomTheme {
    pub name: ThemeName,
    pub extends: crate::theme::BuiltIn,
    pub colors: BTreeMap<Role, Rgb>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub theme: ThemeChoice,
    pub background: Background,
    pub icons: Icons,
    pub mouse: Mouse,
    pub layout: PanelRatios,
    pub grid: GridConfig,
    pub history: HistoryConfig,
    pub log: LogConfig,
    pub driver: DriverConfig,
    pub export: ExportConfig,
    pub themes: Vec<CustomTheme>,
    /// Global order first, then local-only entries; a name in both keeps its
    /// global position.
    pub connections: Vec<ConnectionConfig>,
    pub default_connection: Option<ConnectionName>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Origin(Arc<str>);

impl Origin {
    #[must_use]
    pub fn new(path: &str) -> Self {
        Self(Arc::from(path))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One-based, as an editor counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub origin: Origin,
    /// Absent for an unknown key: the `serde` flatten keeps the name, not the
    /// position.
    pub span: Option<Span>,
    pub severity: Severity,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_is_the_built_in_layer() {
        let config = Config::default();

        assert_eq!(config.grid.max_column_width.get(), 30);
        assert_eq!(config.grid.page_rows.get(), 200);
        assert_eq!(config.layout.sidebar(), 25);
        assert_eq!(config.layout.ddl(), 60);
        assert_eq!(config.layout.queries(), 10);
        assert_eq!(config.icons, Icons::On);
        assert_eq!(config.mouse, Mouse::On);
        assert_eq!(config.background, Background::Theme);
        assert!(config.connections.is_empty());
        assert!(config.default_connection.is_none());
    }

    #[test]
    fn the_clipboard_limit_counts_in_mebibytes() {
        assert_eq!(ClipboardMb::default().bytes(), 10 * 1024 * 1024);
        assert!(ClipboardMb::try_new(0).is_err());
        assert!(ClipboardMb::try_new(1_025).is_err());
    }

    #[test]
    fn every_grid_page_size_is_a_driver_page_size() {
        assert!(PageRows::try_new(100_000).is_ok());
        assert!(PageRows::try_new(100_001).is_err());
        assert!(PageSize::try_new(100_000).is_ok());
        assert_eq!(PageSize::from(PageRows::default()).get(), 200);
    }

    #[test]
    fn a_column_width_of_zero_is_not_constructible() {
        assert!(MaxColumnWidth::try_new(0).is_err());
        assert!(MaxColumnWidth::try_new(1).is_ok());
        assert!(MaxColumnWidth::try_new(512).is_ok());
        assert!(MaxColumnWidth::try_new(513).is_err());
    }

    #[test]
    fn each_panel_ratio_is_validated_against_the_literal_bound_not_layout_floors() {
        assert!(PanelRatios::try_new(25, 60, 10).is_ok());
        assert!(
            PanelRatios::try_new(4, 60, 10).is_err(),
            "5..=95 is the literal bound this type checks, not the floor layout.rs declares"
        );
        assert!(PanelRatios::try_new(96, 60, 10).is_err());
    }

    #[test]
    fn rgb_parses_a_valid_hex_triple() {
        let rgb = Rgb::parse("#1a2b3c").expect("well-formed hex parses");

        assert_eq!((rgb.red, rgb.green, rgb.blue), (0x1a, 0x2b, 0x3c));
    }

    #[test]
    fn rgb_rejects_a_string_the_wrong_length() {
        assert!(Rgb::parse("#1a2b3").is_err());
        assert!(Rgb::parse("#1a2b3cd").is_err());
    }

    #[test]
    fn rgb_rejects_a_string_missing_the_hash() {
        assert!(Rgb::parse("1a2b3c").is_err());
    }

    #[test]
    fn rgb_rejects_a_non_hex_digit() {
        assert!(Rgb::parse("#1a2b3g").is_err());
    }

    #[test]
    fn rgb_rejects_a_leading_plus_a_radix_parse_would_otherwise_accept() {
        assert!(
            Rgb::parse("#+1+2+3").is_err(),
            "u8::from_str_radix accepts a leading '+', which is not a hex digit"
        );
    }

    #[test]
    fn a_connection_name_may_not_be_blank_or_hold_whitespace() {
        assert!(ConnectionName::try_new("prod").is_ok());
        assert!(ConnectionName::try_new("").is_err());
        assert!(ConnectionName::try_new("  ").is_err());
        assert!(ConnectionName::try_new("my prod").is_err());
    }

    #[test]
    fn an_address_shows_what_was_stated() {
        let name = ConnectionName::try_new("c").unwrap();
        assert_eq!(ConnectionConfig::named(name.clone()).address(), "localhost");
        assert_eq!(
            ConnectionConfig {
                host: Some("db".to_owned()),
                port: NonZeroU16::new(6543),
                database: Some("orders".to_owned()),
                ..ConnectionConfig::named(name)
            }
            .address(),
            "db:6543/orders"
        );
    }

    #[test]
    fn a_connection_that_states_no_posture_is_read_only() {
        assert_eq!(Posture::default(), Posture::ReadOnly);
    }

    #[test]
    fn a_diagnostic_quotes_the_file_it_came_from() {
        let diagnostic = Diagnostic {
            origin: Origin::new("./.lazypg.toml"),
            span: Some(Span {
                line: 14,
                column: 11,
            }),
            severity: Severity::Danger,
            message: "connection `dev`: `posture` unknown value \"read\" — entry dropped"
                .to_owned(),
        };

        assert_eq!(diagnostic.origin.as_str(), "./.lazypg.toml");
    }
}
