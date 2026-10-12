use super::AuthError;
use crate::i18n::{self, Locale};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    thread,
    time::{Duration, Instant},
};

pub(super) struct Callback {
    listener: TcpListener,
}

impl Callback {
    pub fn bind() -> Result<Self, AuthError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .map_err(|_| AuthError::Callback)?;
        listener
            .set_nonblocking(true)
            .map_err(|_| AuthError::Callback)?;
        Ok(Self { listener })
    }

    pub fn redirect_uri(&self) -> Result<String, AuthError> {
        Ok(format!(
            "http://{}/oauth/lmm/callback",
            self.listener
                .local_addr()
                .map_err(|_| AuthError::Callback)?
        ))
    }

    pub fn wait(self, issuer: &str, state: &str, timeout: Duration) -> Result<String, AuthError> {
        let started = Instant::now();
        let host = self
            .listener
            .local_addr()
            .map_err(|_| AuthError::Callback)?
            .to_string();
        while started.elapsed() < timeout {
            match self.listener.accept() {
                Ok((mut stream, peer)) if peer.ip().is_loopback() => {
                    let remaining = timeout
                        .saturating_sub(started.elapsed())
                        .min(Duration::from_secs(2));
                    if remaining.is_zero() {
                        break;
                    }
                    let result = read_request(&mut stream, &host, issuer, state, remaining);
                    let response = callback_response(result.is_ok(), i18n::locale());
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(200)));
                    let _ = stream.write_all(response.as_bytes());
                    match result {
                        Ok(code) => return Ok(code),
                        Err(AuthError::Denied) => return Err(AuthError::Denied),
                        Err(_) => continue,
                    }
                }
                Ok(_) => continue,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50))
                }
                Err(_) => return Err(AuthError::Callback),
            }
        }
        Err(AuthError::Timeout)
    }
}

/// Fixed plain-text output; neither authorization codes nor request text are echoed.
fn callback_response(accepted: bool, locale: Locale) -> String {
    let body = if accepted {
        i18n::text(locale, "callback.received")
    } else {
        i18n::text(locale, "callback.invalid")
    };
    let status = if accepted {
        "200 OK"
    } else {
        "400 Bad Request"
    };
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nContent-Language: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; frame-ancestors 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len(),
        locale.tag()
    )
}

fn read_request(
    stream: &mut TcpStream,
    host: &str,
    issuer: &str,
    state: &str,
    budget: Duration,
) -> Result<String, AuthError> {
    let started = Instant::now();
    let mut data = zeroize::Zeroizing::new(Vec::new());
    loop {
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(AuthError::Timeout);
        }
        stream
            .set_read_timeout(Some(remaining))
            .map_err(|_| AuthError::Callback)?;
        let mut chunk = [0u8; 1024];
        let count = stream
            .read(&mut chunk)
            .map_err(|_| AuthError::InvalidResponse)?;
        if count == 0 {
            return Err(AuthError::InvalidResponse);
        }
        data.extend_from_slice(&chunk[..count]);
        if data.len() > 8192 {
            return Err(AuthError::InvalidResponse);
        }
        if data.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
    }
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    let parsed = request
        .parse(&data)
        .map_err(|_| AuthError::InvalidResponse)?;
    if !parsed.is_complete() || request.method != Some("GET") {
        return Err(AuthError::InvalidResponse);
    }
    let hosts: Vec<_> = request
        .headers
        .iter()
        .filter(|header| header.name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1
        || hosts[0].value != host.as_bytes()
        || request.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("transfer-encoding")
                || header.name.eq_ignore_ascii_case("content-length") && header.value != b"0"
        })
    {
        return Err(AuthError::InvalidResponse);
    }
    parse_target(
        request.path.ok_or(AuthError::InvalidResponse)?,
        issuer,
        state,
    )
}

fn parse_target(target: &str, issuer: &str, state: &str) -> Result<String, AuthError> {
    let (path, query) = target.split_once('?').ok_or(AuthError::InvalidResponse)?;
    if path != "/oauth/lmm/callback" || query.contains('#') {
        return Err(AuthError::InvalidResponse);
    }
    let bytes = query.as_bytes();
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'%'
            && (i + 2 >= bytes.len()
                || !bytes[i + 1].is_ascii_hexdigit()
                || !bytes[i + 2].is_ascii_hexdigit())
        {
            return Err(AuthError::InvalidResponse);
        }
    }
    let mut values = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        if !matches!(
            key.as_ref(),
            "code" | "state" | "iss" | "error" | "error_description" | "error_uri"
        ) || value.is_empty()
            || value.chars().any(|c| c.is_control() || c == '\u{fffd}')
            || values
                .insert(key.into_owned(), value.into_owned())
                .is_some()
        {
            return Err(AuthError::InvalidResponse);
        }
    }
    if values.get("state").map(String::as_str) != Some(state)
        || values.get("iss").map(String::as_str) != Some(issuer)
    {
        return Err(AuthError::InvalidResponse);
    }
    if values.contains_key("error") {
        return if values.contains_key("code") {
            Err(AuthError::InvalidResponse)
        } else {
            Err(AuthError::Denied)
        };
    }
    let code = values.remove("code").ok_or(AuthError::InvalidResponse)?;
    if code.len() > 512
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(AuthError::InvalidResponse);
    }
    Ok(code)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn query(extra: &[(&str, &str)]) -> String {
        let mut query = url::form_urlencoded::Serializer::new(String::new());
        query.extend_pairs([("state", "expected"), ("iss", "https://api.lmm.best")]);
        query.extend_pairs(extra.iter().copied());
        format!("/oauth/lmm/callback?{}", query.finish())
    }

    #[test]
    fn callback_translation_uses_utf8_byte_lengths_and_fixed_language_headers() {
        for locale in Locale::ALL {
            for accepted in [true, false] {
                let response = callback_response(accepted, locale);
                let (headers, body) = response.split_once("\r\n\r\n").unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .unwrap()
                    .parse()
                    .unwrap();
                assert_eq!(length, body.len());
                assert!(headers.contains(&format!("Content-Language: {}", locale.tag())));
                assert!(headers.contains("Content-Type: text/plain; charset=utf-8"));
                assert!(headers.contains("Cache-Control: no-store"));
                assert!(headers.contains("Content-Security-Policy: default-src 'none'"));
                assert!(body.contains("LMM CLI"));
                assert!(!body.contains("callback."));
            }
        }
    }

    #[test]
    fn callbacks_reject_mismatched_state_issuer_duplicates_and_malformed_encoding() {
        let good = query(&[("code", "lmm_oa_abc")]);
        assert!(parse_target(&good, "https://api.lmm.best", "expected").is_ok());
        for target in [
            good.replace("expected", "attacker"),
            good.replace("api.lmm.best", "evil.example"),
            format!("{good}&code=other"),
            good.replace("/oauth/lmm/", "/wrong/"),
            format!("{good}&error=%ZZ"),
        ] {
            assert_eq!(
                parse_target(&target, "https://api.lmm.best", "expected"),
                Err(AuthError::InvalidResponse)
            );
        }
        assert_eq!(
            parse_target(
                &query(&[("error", "access_denied")]),
                "https://api.lmm.best",
                "expected"
            ),
            Err(AuthError::Denied)
        );
    }

    #[test]
    fn real_loopback_rejects_wrong_host_then_accepts_bound_callback() {
        let callback = Callback::bind().unwrap();
        let address = callback.listener.local_addr().unwrap();
        let task = thread::spawn(move || {
            callback.wait("https://api.lmm.best", "expected", Duration::from_secs(5))
        });
        for host in ["evil.example".to_string(), address.to_string()] {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            write!(
                stream,
                "GET {} HTTP/1.1\r\nHost: {host}\r\n\r\n",
                query(&[("code", "lmm_oa_abc")])
            )
            .unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.contains("Cache-Control: no-store"));
            assert!(!response.contains("lmm_oa_abc"));
        }
        assert_eq!(task.join().unwrap().unwrap(), "lmm_oa_abc");
    }

    #[test]
    fn missing_browser_callback_times_out_and_releases_port() {
        let callback = Callback::bind().unwrap();
        let address = callback.listener.local_addr().unwrap();
        assert_eq!(
            callback.wait(
                "https://api.lmm.best",
                "expected",
                Duration::from_millis(60)
            ),
            Err(AuthError::Timeout)
        );
        assert!(TcpListener::bind(address).is_ok());
    }
}
