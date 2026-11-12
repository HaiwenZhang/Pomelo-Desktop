//! Net groups inferred from names when the source has none: differential pairs,
//! numbered buses and power or ground nets. Every group says it was inferred.
use crate::model::NetId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPair {
    /// Pair name with the polarity marker removed.
    pub name: String,
    pub positive: NetId,
    pub negative: NetId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bus {
    /// Prefix, index range and suffix, e.g. `DQ[0-15]_A`.
    pub name: String,
    pub members: Vec<NetId>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetGroups {
    pub pairs: Vec<DiffPair>,
    pub buses: Vec<Bus>,
    pub power: Vec<NetId>,
}

/// Polarity markers, positive first. A marker must end the name or a `_` token.
const POLARITY: [(&str, &str); 5] = [
    ("_P", "_N"),
    ("DP", "DN"),
    ("P", "N"),
    ("_T", "_C"),
    ("+", "-"),
];

/// Smallest numbered family that reads as a bus.
const BUS_MIN: usize = 4;

const POWER_PREFIXES: [&str; 16] = [
    "VDD", "VCC", "VSS", "VEE", "GND", "AGND", "DGND", "PGND", "VBAT", "VSYS", "VBUS", "NVCC",
    "AVDD", "DVDD", "VREF", "VPP",
];

fn is_power_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let upper = upper.trim_start_matches(['+', '_']);
    if POWER_PREFIXES
        .iter()
        .any(|prefix| upper.starts_with(prefix))
    {
        return true;
    }
    // Voltage spellings such as 3V3, 1V8 or 5V0 anywhere in a token.
    let bytes = upper.as_bytes();
    bytes
        .windows(3)
        .any(|w| w[0].is_ascii_digit() && w[1] == b'V' && w[2].is_ascii_digit())
        || (upper.ends_with('V')
            && upper[..upper.len() - 1]
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.'))
}

fn split_number(name: &str) -> Option<(&str, &str, &str)> {
    let end = name
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_ascii_digit())
        .map(|(i, c)| i + c.len_utf8())?;
    let start = name[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map(|(i, _)| i)?;
    Some((&name[..start], &name[start..end], &name[end..]))
}

/// Infer groups from net names. `plane_nets` are nets with copper on plane layers,
/// which count as power or ground even without a recognizable name.
pub fn infer(nets: &BTreeMap<NetId, String>, plane_nets: &BTreeSet<NetId>) -> NetGroups {
    let by_name: BTreeMap<&str, NetId> = nets
        .iter()
        .filter(|(id, name)| id.0 != 0 && !name.is_empty())
        .map(|(id, name)| (name.as_str(), *id))
        .collect();
    let mut power: Vec<NetId> = nets
        .iter()
        .filter(|(id, name)| id.0 != 0 && (is_power_name(name) || plane_nets.contains(id)))
        .map(|(id, _)| *id)
        .collect();
    power.sort_unstable();
    let power_set: BTreeSet<_> = power.iter().copied().collect();
    let mut paired = BTreeSet::new();
    let mut pairs = Vec::new();
    for (&name, &id) in &by_name {
        if paired.contains(&id) || power_set.contains(&id) {
            continue;
        }
        'markers: for (positive, negative) in POLARITY {
            for (at, _) in name.match_indices(positive) {
                let after = &name[at + positive.len()..];
                if !(after.is_empty() || after.starts_with('_')) {
                    continue;
                }
                // A bare P or N must follow a letter or digit, not be the whole token.
                if positive == "P" && at == 0 {
                    continue;
                }
                let partner = format!("{}{}{}", &name[..at], negative, after);
                if let Some(&other) = by_name.get(partner.as_str())
                    && other != id
                    && !paired.contains(&other)
                {
                    let base = format!("{}{}", &name[..at], after);
                    let base = base.trim_end_matches('_').to_owned();
                    pairs.push(DiffPair {
                        name: if base.is_empty() {
                            name.to_owned()
                        } else {
                            base
                        },
                        positive: id,
                        negative: other,
                    });
                    paired.insert(id);
                    paired.insert(other);
                    break 'markers;
                }
            }
        }
    }
    let mut families: BTreeMap<(String, String), Vec<(u64, NetId)>> = BTreeMap::new();
    for (&name, &id) in &by_name {
        if power_set.contains(&id) {
            continue;
        }
        if let Some((prefix, number, suffix)) = split_number(name)
            && let Ok(index) = number.parse::<u64>()
            && !prefix.is_empty()
        {
            families
                .entry((prefix.to_owned(), suffix.to_owned()))
                .or_default()
                .push((index, id));
        }
    }
    let mut buses: Vec<_> = families
        .into_iter()
        .filter(|(_, members)| members.len() >= BUS_MIN)
        .map(|((prefix, suffix), mut members)| {
            members.sort_unstable();
            let low = members.first().map_or(0, |m| m.0);
            let high = members.last().map_or(0, |m| m.0);
            Bus {
                name: format!("{prefix}[{low}-{high}]{suffix}"),
                members: members.into_iter().map(|(_, id)| id).collect(),
            }
        })
        .collect();
    buses.sort_by(|a, b| a.name.cmp(&b.name));
    pairs.sort_by(|a, b| a.name.cmp(&b.name));
    NetGroups {
        pairs,
        buses,
        power,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nets(names: &[&str]) -> BTreeMap<NetId, String> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| (NetId(i as u32 + 1), (*name).to_owned()))
            .collect()
    }

    #[test]
    fn pairs_cover_common_polarity_spellings() {
        let nets = nets(&[
            "USB1_DP", "USB1_DN", "PCIE_TXP", "PCIE_TXN", "CK_T_A", "CK_C_A", "LVDS+", "LVDS-",
            "CLK_P", "CLK_N", "PWR_EN",
        ]);
        let groups = infer(&nets, &BTreeSet::new());
        let names: Vec<_> = groups.pairs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["CK_A", "CLK", "LVDS", "PCIE_TX", "USB1"]);
        let ck = groups.pairs.iter().find(|p| p.name == "CK_A").unwrap();
        assert_eq!(nets[&ck.positive], "CK_T_A");
        assert_eq!(nets[&ck.negative], "CK_C_A");
    }

    #[test]
    fn buses_need_four_numbered_members_and_power_is_separate() {
        let mut names: Vec<String> = (0..8).map(|i| format!("DQ{i}_A")).collect();
        names.extend(["DQ0_B", "DQ1_B", "VDD_1V8", "GND", "NVCC_3V3", "SIG"].map(String::from));
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let nets = nets(&refs);
        let plane = BTreeSet::from([*nets.iter().find(|(_, n)| *n == "SIG").unwrap().0]);
        let groups = infer(&nets, &plane);
        assert_eq!(groups.buses.len(), 1);
        assert_eq!(groups.buses[0].name, "DQ[0-7]_A");
        assert_eq!(groups.buses[0].members.len(), 8);
        let power: Vec<_> = groups.power.iter().map(|id| nets[id].as_str()).collect();
        assert_eq!(power, ["VDD_1V8", "GND", "NVCC_3V3", "SIG"]);
    }
}
