//! Código da sala: IP do host + segredo da sala (23 bits) em base32 Crockford,
//! seguidos de 1 caractere de verificação.
//!   IPv4 (32 bits):  11 + 1 = 12 caracteres  XXXX-XXXX-XXXX
//!   IPv6 (128 bits): 31 + 1 = 32 caracteres  XXXX-XXXX-...-XXXX (8 grupos)

use std::net::{Ipv4Addr, Ipv6Addr};
use thiserror::Error;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const SECRET_BITS: u32 = 23;
const V4_DATA_CHARS: usize = 11; // 55 bits = 32 + 23
const V6_DATA_CHARS: usize = 31; // 155 bits >= 128 + 23
pub const ROOM_SECRET_MAX: u32 = 1 << SECRET_BITS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomAddress {
    pub ip: String,
    pub secret: u32,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RoomCodeError {
    #[error("Segredo da sala inválido")]
    InvalidSecret,
    #[error("IP inválido: {0}")]
    InvalidIp(String),
    #[error("O código tem 12 ou 32 caracteres")]
    InvalidLength,
    #[error("Caractere inválido no código: {0}")]
    InvalidCharacter(char),
    #[error("Código incorreto, confira se digitou certo")]
    InvalidChecksum,
}

fn parse_ipv4(ip: &str) -> Result<u32, RoomCodeError> {
    let addr: Ipv4Addr = ip
        .parse()
        .map_err(|_| RoomCodeError::InvalidIp(ip.to_string()))?;
    Ok(u32::from(addr))
}

fn parse_ipv6(ip: &str) -> Result<u128, RoomCodeError> {
    let addr: Ipv6Addr = ip
        .parse()
        .map_err(|_| RoomCodeError::InvalidIp(ip.to_string()))?;
    Ok(u128::from(addr))
}

/// Forma curta (RFC 5952): sem zeros à esquerda e a maior sequência de zeros vira "::".
fn format_ipv6(n: u128) -> String {
    let groups: [u16; 8] = std::array::from_fn(|i| ((n >> (112 - 16 * i)) & 0xffff) as u16);
    let mut best_start: Option<usize> = None;
    let mut best_len = 1usize;
    for i in 0..8 {
        let mut len = 0;
        while i + len < 8 && groups[i + len] == 0 {
            len += 1;
        }
        if len > best_len {
            best_start = Some(i);
            best_len = len;
        }
    }
    let hex: Vec<String> = groups.iter().map(|g| format!("{g:x}")).collect();
    match best_start {
        None => hex.join(":"),
        Some(start) => format!("{}::{}", hex[..start].join(":"), hex[start + best_len..].join(":")),
    }
}

/// Soma ponderada mod 31 (primo), com pesos 1..30 (nunca múltiplos de 31).
fn checksum(values: &[u8]) -> u8 {
    let sum: usize = values
        .iter()
        .enumerate()
        .map(|(i, &v)| ((i % 30) + 1) * usize::from(v))
        .sum();
    (sum % 31) as u8
}

pub fn encode_room_code(addr: &RoomAddress) -> Result<String, RoomCodeError> {
    if addr.secret >= ROOM_SECRET_MAX {
        return Err(RoomCodeError::InvalidSecret);
    }
    let is_v6 = addr.ip.contains(':');
    let mut values = Vec::with_capacity(if is_v6 { V6_DATA_CHARS + 1 } else { V4_DATA_CHARS + 1 });

    if is_v6 {
        let ip_bits = parse_ipv6(&addr.ip)?;
        // 151 bits total (128 IP + 23 secret), encoded into 31 base32 chars (155 bits).
        // Extract 5 bits at a time from least significant bit.
        let secret = u128::from(addr.secret);
        let mut chars = [0u8; V6_DATA_CHARS];
        for (i, slot) in chars.iter_mut().rev().enumerate() {
            let bit_pos = i * 5;
            let val = if bit_pos + 5 <= SECRET_BITS as usize {
                (secret >> bit_pos) & 31
            } else if bit_pos < SECRET_BITS as usize {
                let low_bits = SECRET_BITS as usize - bit_pos;
                let low = secret >> bit_pos;
                let high = (ip_bits & ((1u128 << (5 - low_bits)) - 1)) << low_bits;
                (low | high) & 31
            } else {
                let ip_shift = bit_pos - SECRET_BITS as usize;
                if ip_shift >= 128 {
                    0
                } else {
                    (ip_bits >> ip_shift) & 31
                }
            };
            *slot = val as u8;
        }
        values.extend_from_slice(&chars);
    } else {
        let ip_bits = u64::from(parse_ipv4(&addr.ip)?);
        let mut bits = (ip_bits << SECRET_BITS) | u64::from(addr.secret);
        let mut chars = [0u8; V4_DATA_CHARS];
        for slot in chars.iter_mut().rev() {
            *slot = (bits & 31) as u8;
            bits >>= 5;
        }
        values.extend_from_slice(&chars);
    }

    let check = checksum(&values);
    values.push(check);

    let encoded: String = values.iter().map(|&v| ALPHABET[v as usize] as char).collect();
    let grouped: Vec<&str> = encoded
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("ASCII alphabet"))
        .collect();
    Ok(grouped.join("-"))
}

/// Remove separadores e corrige letras que costumam ser confundidas.
pub fn normalize_room_code(code: &str) -> String {
    code.chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            other => other,
        })
        .collect()
}

/// Formata enquanto digita: XXXX-XXXX-XXXX (IPv4) ou 8 grupos (IPv6).
pub fn format_room_code_input(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .take(32)
        .collect();
    clean
        .as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).expect("ASCII"))
        .collect::<Vec<_>>()
        .join("-")
}

pub fn decode_room_code(code: &str) -> Result<RoomAddress, RoomCodeError> {
    let clean = normalize_room_code(code);
    let is_v6 = clean.len() == V6_DATA_CHARS + 1;
    if !is_v6 && clean.len() != V4_DATA_CHARS + 1 {
        return Err(RoomCodeError::InvalidLength);
    }

    let mut values = Vec::with_capacity(clean.len());
    for c in clean.chars() {
        let pos = ALPHABET
            .iter()
            .position(|&b| b as char == c)
            .ok_or(RoomCodeError::InvalidCharacter(c))?;
        values.push(pos as u8);
    }

    let check = values.pop().ok_or(RoomCodeError::InvalidLength)?;
    if checksum(&values) != check {
        return Err(RoomCodeError::InvalidChecksum);
    }

    if is_v6 {
        // 31 chars = 155 bits. Top 4 bits must be 0 (since 155 - 151 = 4).
        if (values[0] >> 1) != 0 {
            return Err(RoomCodeError::InvalidChecksum);
        }
        // Reconstruct 128-bit IP and 23-bit secret from 31 5-bit values.
        let mut ip_bits: u128 = 0;
        let mut secret: u32 = 0;
        for (idx, &v) in values.iter().enumerate() {
            // Bit offset of the LSB of this 5-bit digit in the 155-bit number
            let lsb = (V6_DATA_CHARS - 1 - idx) * 5;
            for b in 0..5 {
                let bit = (v >> b) & 1;
                if bit == 0 {
                    continue;
                }
                let pos = lsb + b;
                if pos < SECRET_BITS as usize {
                    secret |= 1u32 << pos;
                } else {
                    let ip_pos = pos - SECRET_BITS as usize;
                    if ip_pos >= 128 {
                        return Err(RoomCodeError::InvalidChecksum);
                    }
                    ip_bits |= 1u128 << ip_pos;
                }
            }
        }
        Ok(RoomAddress {
            ip: format_ipv6(ip_bits),
            secret,
        })
    } else {
        let mut bits: u64 = 0;
        for &v in &values {
            bits = (bits << 5) | u64::from(v);
        }
        let ip_bits = (bits >> SECRET_BITS) as u32;
        let secret = (bits & u64::from(ROOM_SECRET_MAX - 1)) as u32;
        Ok(RoomAddress {
            ip: Ipv4Addr::from(ip_bits).to_string(),
            secret,
        })
    }
}

pub fn generate_room_secret() -> u32 {
    rand::random::<u32>() % ROOM_SECRET_MAX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ipv4() {
        let cases = [
            RoomAddress { ip: "0.0.0.0".into(), secret: 0 },
            RoomAddress { ip: "255.255.255.255".into(), secret: ROOM_SECRET_MAX - 1 },
            RoomAddress { ip: "189.45.7.200".into(), secret: 123_456 },
            RoomAddress { ip: "192.168.0.10".into(), secret: 1 },
        ];
        for c in cases {
            let code = encode_room_code(&c).unwrap();
            assert_eq!(code.len(), 14);
            assert_eq!(&code[4..5], "-");
            assert_eq!(&code[9..10], "-");
            assert_eq!(decode_room_code(&code).unwrap(), c);
        }
    }

    #[test]
    fn accepts_lowercase_spaces_and_confusable_letters() {
        let orig = RoomAddress { ip: "10.0.0.1".into(), secret: 42 };
        let code = encode_room_code(&orig).unwrap();
        let messy = code
            .to_lowercase()
            .replace('-', " ")
            .replace('0', "o")
            .replace('1', "l");
        assert_eq!(decode_room_code(&messy).unwrap(), orig);
    }

    #[test]
    fn detects_single_char_substitution_ipv4() {
        let code = normalize_room_code(
            &encode_room_code(&RoomAddress { ip: "177.12.99.3".into(), secret: 7_654_321 }).unwrap(),
        );
        let bytes = code.as_bytes();
        for i in 0..bytes.len() {
            let orig_idx = ALPHABET.iter().position(|&b| b == bytes[i]).unwrap();
            for (cand_idx, &cand) in ALPHABET.iter().enumerate() {
                if cand == bytes[i] || orig_idx.abs_diff(cand_idx) == 31 {
                    continue;
                }
                let mut typo = bytes.to_vec();
                typo[i] = cand;
                assert!(decode_room_code(std::str::from_utf8(&typo).unwrap()).is_err());
            }
        }
    }

    #[test]
    fn detects_adjacent_transposition() {
        let code = normalize_room_code(
            &encode_room_code(&RoomAddress { ip: "200.100.50.25".into(), secret: 99_999 }).unwrap(),
        );
        let bytes = code.as_bytes();
        for i in 0..bytes.len() - 1 {
            if bytes[i] == bytes[i + 1] {
                continue;
            }
            let mut swapped = bytes.to_vec();
            swapped.swap(i, i + 1);
            assert!(decode_room_code(std::str::from_utf8(&swapped).unwrap()).is_err());
        }
    }

    #[test]
    fn rejects_wrong_length() {
        assert_eq!(decode_room_code("ABCD-EFGH"), Err(RoomCodeError::InvalidLength));
    }

    #[test]
    fn roundtrip_ipv6() {
        let cases = [
            RoomAddress { ip: "2804:29b8:5086:d32:6bef:d569:5167:43ee".into(), secret: 4242 },
            RoomAddress { ip: "2001:db8::1".into(), secret: 0 },
            RoomAddress { ip: "::1".into(), secret: ROOM_SECRET_MAX - 1 },
            RoomAddress { ip: "ffff:ffff:ffff:ffff:ffff:ffff:ffff:ffff".into(), secret: 7 },
            RoomAddress { ip: "2804:0:0:1::".into(), secret: 99 },
        ];
        for c in cases {
            let code = encode_room_code(&c).unwrap();
            assert_eq!(code.len(), 39); // 32 chars + 7 dashes
            assert_eq!(decode_room_code(&code).unwrap(), c);
        }
    }

    #[test]
    fn ipv6_long_form_normalizes_to_short() {
        let code = encode_room_code(&RoomAddress {
            ip: "2001:0db8:0000:0000:0000:0000:0000:0001".into(),
            secret: 1,
        })
        .unwrap();
        assert_eq!(decode_room_code(&code).unwrap().ip, "2001:db8::1");
    }

    #[test]
    fn detects_single_char_substitution_ipv6() {
        let code = normalize_room_code(
            &encode_room_code(&RoomAddress {
                ip: "2804:29b8:5086:d32:6bef:d569:5167:43ee".into(),
                secret: 123,
            })
            .unwrap(),
        );
        let bytes = code.as_bytes();
        for i in 0..bytes.len() {
            let orig_idx = ALPHABET.iter().position(|&b| b == bytes[i]).unwrap();
            for (cand_idx, &cand) in ALPHABET.iter().enumerate() {
                if cand == bytes[i] || orig_idx.abs_diff(cand_idx) == 31 {
                    continue;
                }
                let mut typo = bytes.to_vec();
                typo[i] = cand;
                assert!(decode_room_code(std::str::from_utf8(&typo).unwrap()).is_err());
            }
        }
    }

    #[test]
    fn rejects_malformed_ipv6() {
        for bad in ["2001::db8::1", "1:2:3:4:5:6:7", "2001:db8::xyz"] {
            assert!(encode_room_code(&RoomAddress { ip: bad.into(), secret: 1 }).is_err());
        }
    }

    #[test]
    fn generates_secret_in_bounds() {
        for _ in 0..100 {
            let s = generate_room_secret();
            assert!(s < ROOM_SECRET_MAX);
        }
    }
}
