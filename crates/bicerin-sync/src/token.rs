#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncToken(pub i64);

impl SyncToken {
    pub fn new(position: i64) -> Self {
        Self(position)
    }

    pub fn to_string(&self) -> String {
        format!("s{}", self.0)
    }

    pub fn parse(s: &str) -> Option<Self> {
        let token = s.strip_prefix('s')?;
        let position = match token.split_once('~') {
            Some((position, suffix))
                if !suffix.is_empty()
                    && suffix
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') =>
            {
                position
            }
            Some(_) => return None,
            None => token,
        };
        position.parse().ok().map(SyncToken)
    }

    pub fn position(&self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for SyncToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "s{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::SyncToken;

    #[test]
    fn sync_tokens_round_trip_nonnegative_and_negative_positions() {
        for position in [-1, 0, 42] {
            let token = SyncToken::new(position);
            assert_eq!(token.to_string(), format!("s{position}"));
            assert_eq!(SyncToken::parse(&token.to_string()), Some(token));
        }
    }

    #[test]
    fn sync_token_parser_rejects_malformed_tokens() {
        for input in ["", "42", "t42", "s", "s4x", "ss4"] {
            assert_eq!(
                SyncToken::parse(input),
                None,
                "unexpectedly parsed {input:?}"
            );
        }
    }

    #[test]
    fn opaque_sync_batch_suffix_keeps_the_stream_position_parseable() {
        assert_eq!(SyncToken::parse("s42~batch_abc"), Some(SyncToken::new(42)));
        assert_eq!(SyncToken::parse("s42~"), None);
        assert_eq!(SyncToken::parse("s42~not valid"), None);
    }
}
