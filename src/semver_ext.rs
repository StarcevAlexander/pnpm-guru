use node_semver::{Range, Version};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SemverError {
    #[error("invalid version {raw:?}: {source}")]
    InvalidVersion {
        raw: String,
        #[source]
        source: node_semver::SemverError,
    },
    #[error("invalid range {raw:?}: {source}")]
    InvalidRange {
        raw: String,
        #[source]
        source: node_semver::SemverError,
    },
}

pub fn parse_version(raw: &str) -> Result<Version, SemverError> {
    Version::parse(raw).map_err(|source| SemverError::InvalidVersion {
        raw: raw.to_string(),
        source,
    })
}

pub fn parse_range(raw: &str) -> Result<Range, SemverError> {
    Range::parse(raw).map_err(|source| SemverError::InvalidRange {
        raw: raw.to_string(),
        source,
    })
}

fn comparator_matches(version: &Version, comparator: &str) -> bool {
    match Range::parse(comparator.trim()) {
        Ok(range) => range.satisfies(version),
        Err(_) => false,
    }
}

fn or_part_matches(version: &Version, part: &str) -> bool {
    let part = part.trim();
    if let Some((lower, upper)) = part.split_once(" - ") {
        return comparator_matches(version, &format!(">= {lower}"))
            && comparator_matches(version, &format!("<= {upper}"));
    }
    part.split_whitespace()
        .all(|comparator| comparator_matches(version, comparator))
}

pub fn satisfies(version: &Version, range: &str) -> bool {
    range
        .split(" || ")
        .any(|part| or_part_matches(version, part))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_ranges_match() {
        let version = parse_version("1.4.2").unwrap();
        assert!(satisfies(&version, "^1.2.3"));
        assert!(!satisfies(&version, "^1.5.0"));
        assert!(!satisfies(&version, "^2.0.0"));
    }

    #[test]
    fn tilde_locks_minor() {
        let version = parse_version("2.1.9").unwrap();
        assert!(satisfies(&version, "~2.1.0"));
        assert!(!satisfies(&version, "~2.2.0"));
    }

    #[test]
    fn x_ranges_match() {
        assert!(satisfies(&parse_version("1.9.9").unwrap(), "1.x"));
        assert!(satisfies(&parse_version("1.2.0").unwrap(), "1.2.x"));
        assert!(!satisfies(&parse_version("1.3.0").unwrap(), "1.2.x"));
    }

    #[test]
    fn or_of_ranges() {
        let version = parse_version("3.0.0").unwrap();
        assert!(satisfies(&version, "^2.0.0 || ^3.0.0"));
        assert!(!satisfies(&version, "^1.0.0 || ^2.0.0"));
    }

    #[test]
    fn comparators_and_hyphen() {
        assert!(satisfies(
            &parse_version("2.5.0").unwrap(),
            ">=2.0.0 <3.0.0"
        ));
        assert!(satisfies(&parse_version("2.5.0").unwrap(), "2.3.0 - 2.9.9"));
        assert!(!satisfies(
            &parse_version("3.1.0").unwrap(),
            "2.3.0 - 2.9.9"
        ));
    }

    #[test]
    fn invalid_range_is_reported() {
        assert!(parse_range("not-a-range").is_err());
    }
}
