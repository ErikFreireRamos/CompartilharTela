use std::collections::HashSet;

use crate::protocol::RoomMeta;

/// Quem pode assumir como host, em ordem. Todos os apps calculam a mesma lista
/// a partir dos metadados da sala, então chegam ao mesmo sucessor sem conversar.
pub fn successor_candidates<'a, I>(
    meta: Option<&RoomMeta>,
    present: I,
    exclude: &[&str],
) -> Vec<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let Some(meta) = meta else {
        return Vec::new();
    };
    let present_set: HashSet<&str> = present.into_iter().collect();
    let mut excluded: HashSet<&str> = exclude.iter().copied().collect();
    excluded.insert(meta.host_id.as_str());

    meta.queue
        .iter()
        .filter(|id| present_set.contains(id.as_str()) && !excluded.contains(id.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RoomScope;

    fn sample_meta() -> RoomMeta {
        RoomMeta {
            v: 1,
            epoch: 3,
            host_id: "host".into(),
            host_name: "Host".into(),
            code: "XXXX-XXXX-XXXX".into(),
            scope: RoomScope::Internet,
            queue: vec!["ana".into(), "bia".into(), "caio".into()],
            max_quality: None,
        }
    }

    #[test]
    fn follows_queue_order_and_skips_absent() {
        let meta = sample_meta();
        assert_eq!(
            successor_candidates(Some(&meta), ["caio", "ana", "host", "dani"], &[]),
            vec!["ana", "caio"]
        );
    }

    #[test]
    fn never_picks_current_host_or_excluded() {
        let mut meta = sample_meta();
        meta.queue = vec!["host".into(), "ana".into(), "bia".into()];
        assert_eq!(
            successor_candidates(Some(&meta), ["host", "ana", "bia"], &["ana"]),
            vec!["bia"]
        );
    }

    #[test]
    fn empty_without_metadata() {
        assert!(successor_candidates(None, ["ana"], &[]).is_empty());
    }
}
