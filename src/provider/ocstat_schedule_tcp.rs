//! Owner-authorized plaintext schedule reads only. No write commands or reconnect loop.
use super::ProviderError;
use crate::schedule::{Change, DayGroup, Pattern, Schedule};
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};

type Slots = [[u16; 2]; 6];
fn invalid() -> ProviderError {
    ProviderError::InvalidResponse
}
fn hex(bytes: &[u8]) -> Result<u16, ProviderError> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_hexdigit) {
        return Err(invalid());
    }
    u16::from_str_radix(std::str::from_utf8(bytes).map_err(|_| invalid())?, 16)
        .map_err(|_| invalid())
}
fn days(mode: u8) -> Result<Vec<u8>, ProviderError> {
    match mode {
        0 => Ok((1..=7).collect()),
        1 => Ok(vec![8, 9]),
        2 => Ok(vec![10]),
        _ => Err(invalid()),
    }
}
fn registration(user: u64, token: &str) -> Result<String, ProviderError> {
    if user > 9_999_999 || token.len() != 32 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut random = [0; 8];
    getrandom::getrandom(&mut random).map_err(|_| ProviderError::Transport)?;
    let client = 1_000_000_000_000 + u64::from_ne_bytes(random) % 9_000_000_000_000;
    Ok(format!("##3b1{user:07}{client:013}01{token}"))
}
/// Validate identity and all twelve slots before accepting any part of a P frame.
fn decode(frame: &[u8], device: &str, mode: u8) -> Result<[(u8, Slots); 2], ProviderError> {
    if !frame.is_ascii()
        || frame.len() != 98
        || frame.get(..2) != Some(b"##")
        || hex(&frame[2..4])? as usize != frame.len()
        || frame[4] != b'P'
        || &frame[5..17] != device.as_bytes()
        || hex(&frame[20..21])? != u16::from(mode)
        || hex(&frame[19..20])? != 6
        || hex(&frame[93..94]).is_err()
        || hex(&frame[94..]).is_err()
    {
        return Err(invalid());
    }
    let first = hex(&frame[17..18])? as u8;
    let next = hex(&frame[18..19])? as u8;
    let expected = days(mode)?;
    if !expected.contains(&first) || !expected.contains(&next) {
        return Err(invalid());
    }
    let mut rows = [[0; 2]; 12];
    for (i, row) in rows.iter_mut().enumerate() {
        let hour = hex(&frame[45 + i * 4..47 + i * 4])?;
        let minute = hex(&frame[47 + i * 4..49 + i * 4])?;
        let start = hour * 60 + minute;
        let temperature = hex(&frame[21 + i * 2..23 + i * 2])? * 5;
        // 48..71 hours encode disabled slots, not an extra day of active heat.
        if minute >= 60
            || minute % 10 != 0
            || !(hour < 24 || (48..72).contains(&hour))
            || !(50..=350).contains(&temperature)
        {
            return Err(invalid());
        }
        *row = [start, temperature];
    }
    Ok([
        (first, rows[..6].try_into().unwrap()),
        (next, rows[6..].try_into().unwrap()),
    ])
}
fn schedule(mode: u8, slots: BTreeMap<u8, Slots>) -> Result<Schedule, ProviderError> {
    let pattern = match mode {
        0 => Pattern::Weekly,
        1 => Pattern::WeekdayWeekend,
        2 => Pattern::Daily,
        _ => return Err(invalid()),
    };
    let groups = days(mode)?
        .into_iter()
        .zip(pattern.groups())
        .map(|(day, group)| {
            let rows = slots.get(&day).ok_or_else(invalid)?;
            let changes = rows
                .iter()
                .filter(|r| r[0] < 1440)
                .map(|r| {
                    Ok(Change {
                        start_minute: r[0],
                        target_temperature: (r[1] as i16).try_into().map_err(|_| invalid())?,
                    })
                })
                .collect::<Result<Vec<_>, ProviderError>>()?;
            Ok(DayGroup { group, changes })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    Schedule { pattern, groups }
        .validate()
        .map_err(|_| invalid())
}
pub(super) async fn read(
    user: u64,
    token: &str,
    device: &str,
    mode: u8,
) -> Result<Schedule, ProviderError> {
    if device.len() != 12
        || !device
            .bytes()
            .all(|b| b.is_ascii_graphic() && !matches!(b, b'#' | b'&'))
    {
        return Err(invalid());
    }
    let required = days(mode)?;
    let registration = registration(user, token)?;
    timeout(Duration::from_secs(8), async {
        let mut socket = TcpStream::connect(("www.ocstat-thermostat.com", 8800))
            .await
            .map_err(|_| ProviderError::Transport)?;
        exchange(&mut socket, &registration, device, mode, &required).await
    })
    .await
    .map_err(|_| ProviderError::Transport)?
}
async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    socket: &mut S,
    registration: &str,
    device: &str,
    mode: u8,
    required: &[u8],
) -> Result<Schedule, ProviderError> {
    socket
        .write_all(registration.as_bytes())
        .await
        .map_err(|_| ProviderError::Transport)?;
    // The service has no established registration acknowledgement; use the verified delay.
    sleep(Duration::from_secs(1)).await;
    for day in required {
        socket
            .write_all(format!("&&122{device}{day:x}").as_bytes())
            .await
            .map_err(|_| ProviderError::Transport)?;
    }
    let mut slots = BTreeMap::new();
    let mut received = 0;
    loop {
        let mut prefix = [0; 4];
        socket
            .read_exact(&mut prefix)
            .await
            .map_err(|_| ProviderError::Transport)?;
        received += 4;
        if received > 16_384 {
            return Err(invalid());
        }
        if &prefix == b"5502" {
            continue;
        }
        if &prefix[..2] != b"##" {
            return Err(invalid());
        }
        let length = hex(&prefix[2..])? as usize;
        if !(5..=255).contains(&length) {
            return Err(invalid());
        }
        let mut frame = vec![0; length];
        frame[..4].copy_from_slice(&prefix);
        socket
            .read_exact(&mut frame[4..])
            .await
            .map_err(|_| ProviderError::Transport)?;
        received += length - 4;
        if received > 16_384 {
            return Err(invalid());
        }
        if frame[4] != b'P' {
            continue;
        }
        // Ignore notifications for other devices on the same account.
        if frame.get(5..17) != Some(device.as_bytes()) {
            continue;
        }
        for (day, rows) in decode(&frame, device, mode)? {
            if let Some(previous) = slots.insert(day, rows)
                && previous != rows
            {
                return Err(invalid());
            }
        }
        if required.iter().all(|day| slots.contains_key(day)) {
            return schedule(mode, slots);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Vec<u8> {
        // Synthetic identity and slots, preserving the captured two-active/four-disabled shape.
        let rows = [
            [420, 215],
            [1410, 185],
            [3450, 200],
            [3570, 200],
            [3690, 200],
            [3870, 210],
        ];
        let mut s = String::from("##62P(ESTDEVICE019861");
        for _ in 0..2 {
            for row in rows {
                s.push_str(&format!("{:02x}", row[1] / 5));
            }
        }
        for _ in 0..2 {
            for row in rows {
                s.push_str(&format!("{:02x}{:02x}", row[0] / 60, row[0] % 60));
            }
        }
        s.push_str("20000");
        s.into_bytes()
    }
    #[tokio::test(start_paused = true)]
    async fn fragmented_frames_and_heartbeats_are_reassembled_without_writes() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let registration = registration(7, "0123456789abcdef0123456789abcdef").unwrap();
        let serve = async {
            let mut auth = [0; 59];
            server.read_exact(&mut auth).await.unwrap();
            assert_eq!(&auth, registration.as_bytes());
            let mut query = [0; 36];
            server.read_exact(&mut query).await.unwrap();
            assert_eq!(&query, b"&&122(ESTDEVICE018&&122(ESTDEVICE019");
            server.write_all(b"5502").await.unwrap();
            for byte in frame() {
                server.write_all(&[byte]).await.unwrap();
                tokio::task::yield_now().await;
            }
        };
        let read = exchange(&mut client, &registration, "(ESTDEVICE01", 1, &[8, 9]);
        let (result, ()) = tokio::join!(read, serve);
        assert_eq!(result.unwrap().groups.len(), 2);
    }
    #[tokio::test(start_paused = true)]
    async fn truncated_stream_is_not_a_successful_schedule() {
        let (mut client, mut server) = tokio::io::duplex(256);
        let registration = registration(7, "0123456789abcdef0123456789abcdef").unwrap();
        let serve = async {
            let mut outbound = [0; 95];
            server.read_exact(&mut outbound).await.unwrap();
            server.write_all(&frame()[..50]).await.unwrap();
            server.shutdown().await.unwrap();
        };
        let (result, ()) = tokio::join!(
            exchange(&mut client, &registration, "(ESTDEVICE01", 1, &[8, 9]),
            serve
        );
        assert_eq!(result, Err(ProviderError::Transport));
    }
    #[test]
    fn decodes_disabled_slots_and_maps_only_active_periods() {
        let entries = decode(&frame(), "(ESTDEVICE01", 1).unwrap();
        assert_eq!(entries[0].0, 9);
        assert_eq!(entries[1].0, 8);
        assert_eq!(entries[0].1[2], [3450, 200]);
        let s = schedule(1, entries.into_iter().collect()).unwrap();
        assert_eq!(s.groups.len(), 2);
        assert_eq!(s.groups[0].changes.len(), 2);
        assert_eq!(s.groups[0].changes[0].start_minute, 420);
    }
    #[test]
    fn rejects_wrong_identity_mode_length_slots_and_invalid_times() {
        assert!(decode(&frame(), "OTHERDEVICE1", 1).is_err());
        assert!(decode(&frame(), "(ESTDEVICE01", 0).is_err());
        for (index, value) in [
            (2, b'0'),
            (19, b'5'),
            (47, b'f'),
            (45, b'2'),
            (21, b'f'),
            (93, b'g'),
        ] {
            let mut f = frame();
            f[index] = value;
            assert!(decode(&f, "(ESTDEVICE01", 1).is_err());
        }
        for n in 0..frame().len() {
            assert!(decode(&frame()[..n], "(ESTDEVICE01", 1).is_err());
        }
    }
    #[test]
    fn refuses_incomplete_or_empty_group_snapshots() {
        let entries = decode(&frame(), "(ESTDEVICE01", 1).unwrap();
        assert!(schedule(1, [entries[0]].into_iter().collect()).is_err());
        assert!(
            schedule(
                1,
                [(8, [[2880, 185]; 6]), (9, [[2880, 185]; 6])]
                    .into_iter()
                    .collect()
            )
            .is_err()
        );
    }
    #[test]
    fn registration_has_exact_shape_and_validates_inputs() {
        let a = registration(7, "0123456789abcdef0123456789abcdef").unwrap();
        let b = registration(7, "0123456789abcdef0123456789abcdef").unwrap();
        assert_eq!(a.len(), 59);
        assert!(a.starts_with("##3b10000007"));
        assert_ne!(a, b);
        assert!(registration(10_000_000, "0123456789abcdef0123456789abcdef").is_err());
        assert!(registration(7, "not a token").is_err());
    }
}
