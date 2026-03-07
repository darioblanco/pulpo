use rand::RngExt;

const ADJECTIVES: &[&str] = &[
    "abyssal", "amber", "ancient", "azure", "briny", "calm", "coral", "crimson", "crystal", "dark",
    "deep", "drifting", "electric", "emerald", "fierce", "floating", "gentle", "glowing", "golden",
    "hidden", "indigo", "jade", "luminous", "midnight", "misty", "moonlit", "neon", "obsidian",
    "pelagic", "phantom", "radiant", "sapphire", "shadow", "silent", "silver", "spectral",
    "stormy", "swift", "tidal", "twilight", "velvet", "wild",
];

const NOUNS: &[&str] = &[
    "anchor",
    "barnacle",
    "conch",
    "coral",
    "current",
    "drift",
    "fin",
    "gulf",
    "ink",
    "kelp",
    "kraken",
    "lagoon",
    "mantle",
    "maelstrom",
    "nautilus",
    "octopus",
    "oyster",
    "pearl",
    "plankton",
    "polyp",
    "reef",
    "ripple",
    "seabed",
    "shell",
    "shoal",
    "siren",
    "squid",
    "starfish",
    "surge",
    "tentacle",
    "tide",
    "trench",
    "urchin",
    "vortex",
    "wave",
    "whirlpool",
];

/// Generate a random octopus-themed session name (e.g., `crimson-kraken`).
///
/// Checks for collisions via `name_exists` and retries up to 10 times.
/// If all retries collide, appends a numeric suffix (e.g., `crimson-kraken-2`).
pub fn generate_name(name_exists: &dyn Fn(&str) -> bool) -> String {
    let candidates = random_candidates();

    for candidate in &candidates[..10] {
        if !name_exists(candidate) {
            return candidate.clone();
        }
    }

    // Exhausted retries — use the 11th candidate as base and append numeric suffixes
    let base = &candidates[10];
    for suffix in 2..100 {
        let candidate = format!("{base}-{suffix}");
        if !name_exists(&candidate) {
            return candidate;
        }
    }

    // Absolute fallback — UUID-based (should never happen)
    format!("{base}-{}", uuid::Uuid::new_v4().as_simple())
}

fn random_candidates() -> Vec<String> {
    let mut rng = rand::rng();
    (0..11)
        .map(|_| {
            let adj = ADJECTIVES[rng.random_range(0..ADJECTIVES.len())];
            let noun = NOUNS[rng.random_range(0..NOUNS.len())];
            format!("{adj}-{noun}")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_word_lists_non_empty() {
        assert!(!ADJECTIVES.is_empty());
        assert!(!NOUNS.is_empty());
    }

    #[test]
    fn test_word_lists_are_kebab_safe() {
        for word in ADJECTIVES.iter().chain(NOUNS.iter()) {
            assert!(
                word.chars().all(|c| c.is_ascii_lowercase()),
                "word {word:?} contains non-lowercase-ascii chars"
            );
            assert!(!word.contains('-'), "word {word:?} contains a hyphen");
        }
    }

    #[test]
    fn test_combination_count() {
        let total = ADJECTIVES.len() * NOUNS.len();
        assert!(
            total >= 1000,
            "only {total} combinations — need at least 1000"
        );
    }

    #[test]
    fn test_generate_name_format() {
        let name = generate_name(&|_| false);
        let parts: Vec<&str> = name.split('-').collect();
        assert_eq!(parts.len(), 2, "expected adjective-noun, got {name}");
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }

    #[test]
    fn test_generate_name_no_collision() {
        let name = generate_name(&|_| false);
        assert!(!name.is_empty());
    }

    #[test]
    fn test_generate_name_avoids_collisions() {
        let mut taken = HashSet::new();
        taken.insert("deep-kraken".to_owned());

        let name = generate_name(&|candidate| taken.contains(candidate));
        assert_ne!(name, "deep-kraken");
    }

    #[test]
    fn test_generate_name_suffix_on_exhausted_retries() {
        // All base names "collide" — only suffixed names pass
        let name = generate_name(&|candidate| !candidate.ends_with("-2"));

        assert!(name.ends_with("-2"), "expected suffix, got {name}");
        assert_eq!(name.split('-').count(), 3);
    }

    #[test]
    fn test_generate_name_uuid_fallback() {
        // Everything collides — force the UUID fallback
        let name = generate_name(&|_| true);
        // Should end with a UUID (32 hex chars)
        let last_dash = name.rfind('-').unwrap();
        let suffix = &name[last_dash + 1..];
        assert_eq!(suffix.len(), 32, "expected UUID suffix, got {name}");
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_generate_name_uniqueness() {
        let mut names = HashSet::new();
        for _ in 0..50 {
            let name = generate_name(&|_| false);
            names.insert(name);
        }
        // With 1500+ combinations, 50 names should have very few collisions
        let unique = names.len();
        assert!(
            unique >= 40,
            "too many collisions: only {unique} unique out of 50"
        );
    }
}
