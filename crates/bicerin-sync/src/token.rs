#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncToken(pub i64);

impl SyncToken {
    pub fn new(position: i64) -> Self { Self(position) }

    pub fn to_string(&self) -> String {
        format!("s{}", self.0)
    }

    pub fn parse(s: &str) -> Option<Self> {
        s.strip_prefix('s').and_then(|n| n.parse().ok()).map(SyncToken)
    }

    pub fn position(&self) -> i64 { self.0 }
}

impl std::fmt::Display for SyncToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}
