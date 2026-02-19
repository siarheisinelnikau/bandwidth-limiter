//! Bounded TCP header parsing. Unsupported packets are forwarded without a shaping claim.
#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
pub struct FlowKey {
    pub local: [u32; 4],
    pub remote: [u32; 4],
    pub local_port: u16,
    pub remote_port: u16,
    pub ipv6: bool,
}
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub key: FlowKey,
    pub payload: usize,
    pub direction: usize,
}
fn words(b: &[u8]) -> [u32; 4] {
    std::array::from_fn(|i| u32::from_be_bytes(b[(3 - i) * 4..(4 - i) * 4].try_into().unwrap()))
}
pub fn parse(b: &[u8], outbound: bool) -> Option<Info> {
    let (src, dst, ipv6, mut offset, mut protocol, end) = match b.first()? >> 4 {
        4 => {
            if b.len() < 20 {
                return None;
            }
            let end = u16::from_be_bytes([b[2], b[3]]) as usize;
            let offset = usize::from(b[0] & 15) * 4;
            if end != b.len()
                || offset < 20
                || offset > end
                || u16::from_be_bytes([b[6], b[7]]) & 0x3fff != 0
            {
                return None;
            }
            (
                [u32::from_be_bytes(b[12..16].try_into().ok()?), 0xffff, 0, 0],
                [u32::from_be_bytes(b[16..20].try_into().ok()?), 0xffff, 0, 0],
                false,
                offset,
                b[9],
                end,
            )
        }
        6 => {
            if b.len() < 40 {
                return None;
            }
            let end = 40 + u16::from_be_bytes([b[4], b[5]]) as usize;
            if end != b.len() || end == 40 {
                return None;
            }
            // FLOW has no interface scope. Do not conflate equal link-local addresses on adapters.
            for a in [&b[8..24], &b[24..40]] {
                if a[0] == 0xff || (a[0] == 0xfe && a[1] & 0xc0 == 0x80) {
                    return None;
                }
            }
            (words(&b[8..24]), words(&b[24..40]), true, 40, b[6], end)
        }
        _ => return None,
    };
    for _ in 0..8 {
        if protocol == 6 {
            break;
        }
        if !ipv6 || !matches!(protocol, 0 | 43 | 60 | 51) || offset + 2 > end {
            return None;
        }
        let length = if protocol == 51 {
            (usize::from(b[offset + 1]) + 2) * 4
        } else {
            (usize::from(b[offset + 1]) + 1) * 8
        };
        protocol = b[offset];
        offset = offset.checked_add(length)?;
        if offset > end {
            return None;
        }
    }
    if protocol != 6 || offset + 20 > end {
        return None;
    }
    let tcp = usize::from(b[offset + 12] >> 4) * 4;
    if tcp < 20 || offset + tcp > end {
        return None;
    }
    let sp = u16::from_be_bytes([b[offset], b[offset + 1]]);
    let dp = u16::from_be_bytes([b[offset + 2], b[offset + 3]]);
    let (local, remote, local_port, remote_port) = if outbound {
        (src, dst, sp, dp)
    } else {
        (dst, src, dp, sp)
    };
    Some(Info {
        key: FlowKey {
            local,
            remote,
            local_port,
            remote_port,
            ipv6,
        },
        payload: end - offset - tcp,
        direction: usize::from(outbound),
    })
}
#[cfg(test)]
pub fn fixture(payload: usize, outbound: bool, port: u16) -> Vec<u8> {
    let mut b = vec![0; 40 + payload];
    b[0] = 0x45;
    b[9] = 6;
    b[32] = 0x50;
    b[2..4].copy_from_slice(&((40 + payload) as u16).to_be_bytes());
    let (src, dst, sp, dp) = if outbound {
        ([10, 0, 0, 1], [10, 0, 0, 2], port, 443)
    } else {
        ([10, 0, 0, 2], [10, 0, 0, 1], 443, port)
    };
    b[12..16].copy_from_slice(&src);
    b[16..20].copy_from_slice(&dst);
    b[20..22].copy_from_slice(&sp.to_be_bytes());
    b[22..24].copy_from_slice(&dp.to_be_bytes());
    b
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ipv4_directions_options_and_invalid_lengths() {
        let b = fixture(100, true, 1234);
        let up = parse(&b, true).unwrap();
        let down = parse(&fixture(50, false, 1234), false).unwrap();
        assert_eq!(up.key, down.key);
        assert_eq!((up.payload, down.payload), (100, 50));
        let mut options = b.clone();
        options.splice(20..20, [0; 4]);
        options[0] = 0x46;
        options[2..4].copy_from_slice(&144u16.to_be_bytes());
        assert_eq!(parse(&options, true).unwrap().payload, 100);
        let mut bad = b.clone();
        bad[6] = 0x20;
        assert!(parse(&bad, true).is_none());
        bad = b.clone();
        bad[32] = 0xf0;
        bad.truncate(50);
        assert!(parse(&bad, true).is_none());
        for size in 0..40 {
            assert!(parse(&b[..size], true).is_none());
        }
    }
    #[test]
    fn ipv6_extension_chain_host_words_and_scope_exclusion() {
        let mut b = vec![0; 76];
        b[0] = 0x60;
        b[4..6].copy_from_slice(&36u16.to_be_bytes());
        b[6] = 60;
        b[40] = 6;
        b[60] = 0x50;
        b[8..24].copy_from_slice(&[0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        b[24..40].copy_from_slice(&[0x20, 1, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
        let p = parse(&b, true).unwrap();
        assert_eq!(p.key.local, [1, 0, 0, 0x20010db8]);
        assert_eq!(p.payload, 8);
        b[6] = 44;
        assert!(parse(&b, true).is_none());
        b[6] = 60;
        b[8] = 0xfe;
        b[9] = 0x80;
        assert!(parse(&b, true).is_none());
    }
}
