//! Incremental size guard, not a JSON repair/parser. serde_json validates the
//! complete envelope afterward. Only /choices/0/message/content is counted as
//! contracted output; the entire HTTP envelope also has a finite size limit.
use super::types::ProviderError;

enum Frame {
    Object {
        path: Vec<String>,
        key: String,
        expecting_key: bool,
    },
    Array {
        path: Vec<String>,
        index: usize,
    },
}

pub(super) struct ContentGuard {
    frames: Vec<Frame>,
    in_string: bool,
    key: Option<Vec<u8>>,
    bounded: bool,
    escaped: bool,
    unicode: Option<(u16, u8)>,
    high_surrogate: bool,
    primitive: bool,
    content_bytes: usize,
    limit: usize,
}

impl ContentGuard {
    pub fn new(limit: usize) -> Self {
        Self {
            frames: vec![],
            in_string: false,
            key: None,
            bounded: false,
            escaped: false,
            unicode: None,
            high_surrogate: false,
            primitive: false,
            content_bytes: 0,
            limit,
        }
    }
    fn path(&self) -> Vec<String> {
        match self.frames.last() {
            Some(Frame::Object { path, key, .. }) => {
                let mut path = path.clone();
                path.push(key.clone());
                path
            }
            Some(Frame::Array { path, index }) => {
                let mut path = path.clone();
                path.push(index.to_string());
                path
            }
            None => vec![],
        }
    }
    fn complete_value(&mut self) {
        if let Some(Frame::Array { index, .. }) = self.frames.last_mut() {
            *index += 1;
        }
    }
    fn count(&mut self, bytes: usize) -> Result<(), ProviderError> {
        if self.bounded {
            self.content_bytes = self.content_bytes.saturating_add(bytes);
            if self.content_bytes > self.limit {
                return Err(ProviderError::OutputLimitExceeded);
            }
        }
        Ok(())
    }
    pub fn push(&mut self, bytes: &[u8]) -> Result<(), ProviderError> {
        for &byte in bytes {
            if self.in_string {
                if let Some(key) = self.key.as_mut() {
                    // Object keys are never output. Bound pathological metadata keys.
                    if key.len() >= 1024 {
                        return Err(ProviderError::Protocol);
                    }
                    key.push(byte);
                }
                if let Some((value, digits)) = self.unicode {
                    let hex = (byte as char).to_digit(16).ok_or(ProviderError::Protocol)? as u16;
                    let value = (value << 4) | hex;
                    if digits == 3 {
                        self.unicode = None;
                        if self.bounded {
                            match value {
                                0xD800..=0xDBFF if !self.high_surrogate => {
                                    self.high_surrogate = true
                                }
                                0xDC00..=0xDFFF if self.high_surrogate => {
                                    self.high_surrogate = false;
                                    self.count(4)?;
                                }
                                0xD800..=0xDFFF => return Err(ProviderError::Protocol),
                                _ if self.high_surrogate => return Err(ProviderError::Protocol),
                                0..=0x7F => self.count(1)?,
                                0x80..=0x7FF => self.count(2)?,
                                _ => self.count(3)?,
                            }
                        }
                    } else {
                        self.unicode = Some((value, digits + 1));
                    }
                    continue;
                }
                if self.escaped {
                    self.escaped = false;
                    if byte == b'u' {
                        self.unicode = Some((0, 0));
                    } else {
                        if !matches!(byte, b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't')
                            || (self.bounded && self.high_surrogate)
                        {
                            return Err(ProviderError::Protocol);
                        }
                        self.count(1)?;
                    }
                    continue;
                }
                match byte {
                    b'\\' => self.escaped = true,
                    b'"' => {
                        if self.bounded && self.high_surrogate {
                            return Err(ProviderError::Protocol);
                        }
                        self.in_string = false;
                        if let Some(key) = self.key.take() {
                            let key: String = serde_json::from_slice(&key)
                                .map_err(|_| ProviderError::Protocol)?;
                            if let Some(Frame::Object { key: slot, .. }) = self.frames.last_mut() {
                                *slot = key;
                            }
                        } else {
                            self.complete_value();
                        }
                    }
                    0..=31 => return Err(ProviderError::Protocol),
                    _ => {
                        if self.bounded && self.high_surrogate {
                            return Err(ProviderError::Protocol);
                        }
                        self.count(1)?;
                    }
                }
                continue;
            }
            if self.primitive {
                if !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']' | b'}') {
                    continue;
                }
                self.primitive = false;
                self.complete_value();
            }
            match byte {
                b'"' => {
                    self.in_string = true;
                    self.escaped = false;
                    self.high_surrogate = false;
                    let is_key = matches!(
                        self.frames.last(),
                        Some(Frame::Object {
                            expecting_key: true,
                            ..
                        })
                    );
                    self.key = is_key.then(|| vec![b'"']);
                    self.bounded = !is_key && self.path() == ["choices", "0", "message", "content"];
                    // Count all duplicate occurrences too; malformed envelopes cannot bypass the bound.
                }
                b'{' | b'[' => {
                    if self.frames.len() >= 128 {
                        return Err(ProviderError::Protocol);
                    }
                    let path = self.path();
                    self.frames.push(if byte == b'{' {
                        Frame::Object {
                            path,
                            key: String::new(),
                            expecting_key: true,
                        }
                    } else {
                        Frame::Array { path, index: 0 }
                    });
                }
                b'}' | b']' => {
                    self.frames.pop().ok_or(ProviderError::Protocol)?;
                    self.complete_value();
                }
                b':' => {
                    if let Some(Frame::Object { expecting_key, .. }) = self.frames.last_mut() {
                        *expecting_key = false;
                    }
                }
                b',' => {
                    if let Some(Frame::Object { expecting_key, .. }) = self.frames.last_mut() {
                        *expecting_key = true;
                    }
                }
                b if b.is_ascii_whitespace() => {}
                _ => self.primitive = true,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fix5_byte_guard_counts_utf8_and_json_escapes_across_every_boundary() {
        for text in ["", "á😀xy", "quote\"slash\\newline\n", "\u{0000}", "日本語"] {
            let body = serde_json::json!({"ignored":{"content":"not output"},"choices":[{"message":{"reasoning":"ignored", "content":text},"finish_reason":"stop"}]}).to_string();
            let mut guard = ContentGuard::new(text.len().max(1));
            for byte in body.bytes() {
                guard.push(&[byte]).unwrap();
            }
            assert_eq!(guard.content_bytes, text.len());
            if !text.is_empty() {
                let mut smaller = ContentGuard::new(text.len() - 1);
                assert_eq!(
                    smaller.push(body.as_bytes()),
                    Err(ProviderError::OutputLimitExceeded)
                );
            }
        }
        let escaped = br#"{"choices":[{"message":{"\u0063ontent":"\u00e1\ud83d\ude00xy"}}]}"#;
        let mut guard = ContentGuard::new(8);
        for byte in escaped {
            guard.push(&[*byte]).unwrap();
        }
        assert_eq!(guard.content_bytes, 8);
        assert_eq!(
            ContentGuard::new(7).push(escaped),
            Err(ProviderError::OutputLimitExceeded)
        );
    }
    #[test]
    fn fix5_byte_guard_rejects_invalid_surrogates_and_bounds_duplicate_content() {
        for body in [
            br#"{"choices":[{"message":{"content":"\ud83dx"}}]}"#.as_slice(),
            br#"{"choices":[{"message":{"content":"\ude00"}}]}"#.as_slice(),
        ] {
            assert_eq!(
                ContentGuard::new(100).push(body),
                Err(ProviderError::Protocol)
            );
        }
        assert_eq!(
            ContentGuard::new(3)
                .push(br#"{"choices":[{"message":{"content":"xx","content":"xx"}}]}"#),
            Err(ProviderError::OutputLimitExceeded)
        );
    }
}
