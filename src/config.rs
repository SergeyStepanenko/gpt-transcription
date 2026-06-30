use std::collections::HashMap;
use std::path::Path;

pub struct Creds {
    pub token: String,
    pub account_id: String,
    pub cookies: String,
}

pub fn load_creds(dir: &Path) -> Creds {
    let path = dir.join("creds.env");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut env = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (k, v) = line.split_once('=').unwrap();
        let v = v.trim().trim_matches('\'').trim_matches('"');
        env.insert(k.trim().to_string(), v.to_string());
    }

    let get = |key: &str| -> String {
        env.get(key)
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| panic!("{key} is not set in creds.env"))
            .clone()
    };

    Creds {
        token: get("TOKEN"),
        account_id: get("ACCOUNT_ID"),
        cookies: get("COOKIES"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_creds_env() {
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join("creds.env")).unwrap();
        writeln!(f, "# comment").unwrap();
        writeln!(f, "TOKEN='tok123'").unwrap();
        writeln!(f, "ACCOUNT_ID=\"acc456\"").unwrap();
        writeln!(f, "COOKIES=c=1; d=2").unwrap();
        drop(f);

        let c = load_creds(dir.path());
        assert_eq!(c.token, "tok123");
        assert_eq!(c.account_id, "acc456");
        assert_eq!(c.cookies, "c=1; d=2");
    }

    #[test]
    #[should_panic(expected = "TOKEN is not set")]
    fn missing_key_panics() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("creds.env"), "ACCOUNT_ID=x\nCOOKIES=y\n").unwrap();
        load_creds(dir.path());
    }
}
