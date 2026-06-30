use std::collections::HashMap;
use std::io::{self, BufRead};
use std::path::Path;

pub struct Creds {
    pub token: String,
    pub account_id: String,
    pub cookies: String,
}

pub fn has_usable_creds(dir: &Path) -> bool {
    try_load_creds(dir).is_ok()
}

pub fn load_or_create_creds(dir: &Path) -> Creds {
    if let Ok(creds) = try_load_creds(dir) {
        return creds;
    }

    replace_creds_from_curl(dir)
}

pub fn replace_creds_from_curl(dir: &Path) -> Creds {
    if !atty::is(atty::Stream::Stdin) {
        panic!(
            "creds.env is missing or incomplete. Run interactively once, or create creds.env with TOKEN, ACCOUNT_ID, COOKIES."
        );
    }

    loop {
        print_curl_instructions();

        let curl = read_stdin_to_string();
        match creds_from_curl(&curl) {
            Ok(creds) => {
                write_creds_env(&dir.join("creds.env"), &creds)
                    .unwrap_or_else(|e| panic!("cannot write creds.env: {e}"));
                println!("Saved credentials to {}", dir.join("creds.env").display());
                return creds;
            }
            Err(e) => {
                eprintln!("cannot extract credentials: {e}");
                eprintln!(
                    "Try again: copy a fresh Chrome DevTools cURL from a chatgpt.com/backend-api request and paste it here."
                );
            }
        }
    }
}

#[cfg(test)]
pub fn load_creds(dir: &Path) -> Creds {
    try_load_creds(dir).unwrap_or_else(|e| panic!("{e}"))
}

fn try_load_creds(dir: &Path) -> Result<Creds, String> {
    let path = dir.join("creds.env");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    creds_from_env(&text).map_err(|e| format!("{e} in {}", path.display()))
}

fn creds_from_env(text: &str) -> Result<Creds, String> {
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

    let get = |key: &str| -> Result<String, String> {
        env.get(key)
            .filter(|v| !v.is_empty())
            .cloned()
            .ok_or_else(|| format!("{key} is not set"))
    };

    Ok(Creds {
        token: get("TOKEN")?,
        account_id: get("ACCOUNT_ID")?,
        cookies: get("COOKIES")?,
    })
}

fn read_stdin_to_string() -> String {
    let stdin = io::stdin();
    let mut text = String::new();
    for line in stdin.lock().lines() {
        text.push_str(&line.unwrap_or_default());
        text.push('\n');
    }
    text
}

fn print_curl_instructions() {
    println!("Credentials are missing, incomplete, or need replacement.");
    println!();
    println!("How to get them:");
    println!("1. Open https://chatgpt.com in Chrome while logged in.");
    println!("2. Open DevTools -> Network.");
    println!("3. Use ChatGPT once, or record a short voice input.");
    println!("4. Pick any request whose URL starts with https://chatgpt.com/backend-api.");
    println!("5. Right-click it -> Copy -> Copy as cURL.");
    println!("6. Paste the whole cURL below, then press Ctrl-D.");
    println!();
}

fn creds_from_curl(curl: &str) -> Result<Creds, String> {
    let args = curl_args(curl).map_err(|e| format!("cannot parse copied cURL: {e}"))?;
    let auth = extract_header(&args, "authorization")
        .ok_or_else(|| "missing authorization: Bearer header".to_string())?;
    let token = auth
        .strip_prefix("Bearer ")
        .or_else(|| auth.strip_prefix("bearer "))
        .ok_or_else(|| "authorization header is not Bearer token".to_string())?
        .trim()
        .to_string();

    let account_id = extract_header(&args, "chatgpt-account-id")
        .or_else(|| account_id_from_jwt(&token))
        .ok_or_else(|| {
            "missing chatgpt-account-id header and no chatgpt_account_id claim in token".to_string()
        })?;
    let cookies = extract_cookies(&args)
        .ok_or_else(|| "missing cookies: expected -b, --cookie, or cookie header".to_string())?;

    Ok(Creds {
        token,
        account_id,
        cookies,
    })
}

fn extract_header(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let header = match option_value(arg, &mut iter, "-H", "--header") {
            Some(header) => header,
            None => continue,
        };
        let Some((k, v)) = header.split_once(':') else {
            continue;
        };
        if k.trim().eq_ignore_ascii_case(name) {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn extract_cookies(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(cookie) = option_value(arg, &mut iter, "-b", "--cookie") {
            return Some(cookie.to_string());
        }
    }
    extract_header(args, "cookie")
}

fn option_value<'a, I>(arg: &'a str, iter: &mut I, short: &str, long: &str) -> Option<&'a str>
where
    I: Iterator<Item = &'a String>,
{
    if arg == short || arg == long {
        return iter.next().map(String::as_str);
    }
    if arg.starts_with(short) && arg.len() > short.len() {
        return Some(&arg[short.len()..]);
    }
    arg.strip_prefix(&format!("{long}="))
}

fn curl_args(curl: &str) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_word = false;
    let mut quote = None;
    let mut escape = false;
    let mut chars = curl.chars().peekable();

    while let Some(c) = chars.next() {
        if escape {
            if c != '\n' {
                current.push(c);
                in_word = true;
            }
            escape = false;
            continue;
        }

        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    current.push(c);
                }
            }
            Some('"') => match c {
                '"' => quote = None,
                '\\' => escape = true,
                _ => current.push(c),
            },
            None => match c {
                '\\' => escape = true,
                '\'' | '"' => {
                    quote = Some(c);
                    in_word = true;
                }
                '$' if chars.peek() == Some(&'\'') => {
                    chars.next();
                    quote = Some('\'');
                    in_word = true;
                }
                c if c.is_whitespace() => {
                    if in_word {
                        args.push(std::mem::take(&mut current));
                        in_word = false;
                    }
                }
                _ => {
                    current.push(c);
                    in_word = true;
                }
            },
            _ => unreachable!(),
        }
    }

    if let Some(q) = quote {
        return Err(format!("unterminated {q} quote"));
    }
    if escape {
        current.push('\\');
        in_word = true;
    }
    if in_word {
        args.push(current);
    }

    Ok(args)
}

fn account_id_from_jwt(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let payload = base64url_decode(payload).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    json.get("https://api.openai.com/auth")
        .and_then(|v| v.get("chatgpt_account_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, ()> {
    if input.len() % 4 == 1 {
        return Err(());
    }

    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u8;

    for b in input.bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            b'=' => break,
            _ => return Err(()),
        } as u32;

        buf = (buf << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((buf >> bits) & 0xff) as u8);
        }
    }

    Ok(out)
}

fn write_creds_env(path: &Path, creds: &Creds) -> std::io::Result<()> {
    let text = format!(
        "TOKEN={}\nACCOUNT_ID={}\nCOOKIES={}\n",
        shell_quote(&creds.token),
        shell_quote(&creds.account_id),
        shell_quote(&creds.cookies)
    );
    std::fs::write(path, text)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn token_with_account(account_id: &str) -> String {
        let payload =
            format!(r#"{{"https://api.openai.com/auth":{{"chatgpt_account_id":"{account_id}"}}}}"#);
        format!("h.{}.s", base64url_encode(payload.as_bytes()))
    }

    fn base64url_encode(input: &[u8]) -> String {
        const TABLE: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0];
            let b1 = *chunk.get(1).unwrap_or(&0);
            let b2 = *chunk.get(2).unwrap_or(&0);
            let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;

            out.push(TABLE[((n >> 18) & 63) as usize] as char);
            out.push(TABLE[((n >> 12) & 63) as usize] as char);
            if chunk.len() > 1 {
                out.push(TABLE[((n >> 6) & 63) as usize] as char);
            }
            if chunk.len() > 2 {
                out.push(TABLE[(n & 63) as usize] as char);
            }
        }
        out
    }

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

    #[test]
    fn extract_creds_from_curl_with_jwt_account_fallback() {
        let token = token_with_account("acc-from-jwt");
        let curl = [
            "curl x \\",
            &format!("  -H 'authorization: Bearer {token}' \\"),
            "  -b 'a=1; b=two'",
        ]
        .join("\n");
        let c = creds_from_curl(&curl).unwrap();
        assert_eq!(c.token, token);
        assert_eq!(c.account_id, "acc-from-jwt");
        assert_eq!(c.cookies, "a=1; b=two");
    }

    #[test]
    fn explicit_account_header_wins() {
        let token = token_with_account("acc-from-jwt");
        let curl = [
            "curl x \\",
            &format!("  -H 'authorization: Bearer {token}' \\"),
            "  -H 'chatgpt-account-id: acc-header' \\",
            "  -H 'cookie: c=3'",
        ]
        .join("\n");
        let c = creds_from_curl(&curl).unwrap();
        assert_eq!(c.account_id, "acc-header");
        assert_eq!(c.cookies, "c=3");
    }

    #[test]
    fn extract_creds_from_chrome_copy_as_curl_shape() {
        let token = token_with_account("acc-from-jwt");
        let curl = [
            "curl --url 'https://chatgpt.com/backend-api/sentinel/ping' \\",
            "  -X 'POST' \\",
            "  -H 'accept: */*' \\",
            &format!("  -H 'authorization: Bearer {token}' \\"),
            "  -b '__Host-next-auth.csrf-token=csrf; cf_clearance=clearance; other=1' \\",
            "  -H 'origin: https://chatgpt.com' \\",
            "  -H 'referer: https://chatgpt.com/'",
        ]
        .join("\n");
        let c = creds_from_curl(&curl).unwrap();
        assert_eq!(c.token, token);
        assert_eq!(c.account_id, "acc-from-jwt");
        assert_eq!(
            c.cookies,
            "__Host-next-auth.csrf-token=csrf; cf_clearance=clearance; other=1"
        );
    }

    #[test]
    fn extract_creds_from_single_line_curl() {
        let token = token_with_account("acc-from-jwt");
        let curl = format!(
            "curl 'https://chatgpt.com/backend-api/x' -H 'authorization: Bearer {token}' --cookie 'a=1; b=two'"
        );
        let c = creds_from_curl(&curl).unwrap();
        assert_eq!(c.token, token);
        assert_eq!(c.cookies, "a=1; b=two");
    }

    #[test]
    fn extract_creds_from_local_curl_fixture_if_present() {
        let path = Path::new("curl");
        if !path.exists() {
            return;
        }

        let curl = std::fs::read_to_string(path).unwrap();
        let c = creds_from_curl(&curl).unwrap();
        assert!(!c.token.is_empty());
        assert!(!c.account_id.is_empty());
        assert!(!c.cookies.is_empty());
    }
}
