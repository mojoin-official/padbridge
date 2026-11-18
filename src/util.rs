//! Small string helpers shared by the name tables and the config parser.

/// Normalises a user-typed name for comparison: lower-case, with `_`, `-` and spaces removed,
/// so `"Left_Stick Up"`, `"left-stick-up"` and `"LeftStickUp"` are the same name.
pub fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

/// The file-name part of a Windows or Unix path.
pub fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Whether the running program `exe` (a path or file name) is the one the user named. Only file
/// names are compared, case-insensitively, and ".exe" may be left off `wanted`.
pub fn exe_matches(exe: &str, wanted: &str) -> bool {
    let have = file_name(exe.trim()).to_lowercase();
    let want = file_name(wanted.trim()).to_lowercase();
    if want.is_empty() {
        return false;
    }
    have == want || have.strip_suffix(".exe") == Some(want.as_str())
}

/// Levenshtein distance over chars.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Returns the candidate closest to `input` if it is close enough to be a plausible typo.
pub fn suggest<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let n = norm(input);
    let limit = (n.chars().count() / 3).max(2);
    candidates
        .into_iter()
        .map(|c| (levenshtein(&n, &norm(c)), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_ignores_case_and_separators() {
        assert_eq!(norm("Left_Stick Up"), "leftstickup");
        assert_eq!(norm("left-stick-up"), "leftstickup");
    }

    #[test]
    fn program_names_match_loosely() {
        assert!(exe_matches("C:\\Games\\Foo\\Game.EXE", "game.exe"));
        assert!(exe_matches("C:\\Games\\Foo\\game.exe", "Game"));
        assert!(exe_matches("game.exe", "D:\\other\\place\\GAME.exe"));
        assert!(!exe_matches("C:\\x\\gamelauncher.exe", "game.exe"));
        assert!(!exe_matches("C:\\x\\game.exe", ""));
        assert!(!exe_matches("C:\\x\\game.exe", "   "));
        assert_eq!(file_name("a/b\\c.exe"), "c.exe");
    }

    #[test]
    fn levenshtein_basics() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("same", "same"), 0);
    }

    #[test]
    fn suggests_close_names_only() {
        let names = ["Space", "Enter", "Backspace"];
        assert_eq!(suggest("Spase", names.iter().copied()), Some("Space"));
        assert_eq!(suggest("zzzzzzzz", names.iter().copied()), None);
    }
}
