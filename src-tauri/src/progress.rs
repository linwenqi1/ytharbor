#[derive(Debug, Default)]
pub struct ParsedProgress {
    pub percent: Option<f64>,
    pub speed: Option<f64>,
    pub eta: Option<u64>,
}

pub fn parse_progress(line: &str) -> Option<ParsedProgress> {
    let payload = line.strip_prefix("ytharbor-progress:")?;
    let mut fields = payload.split('|');
    let downloaded = parse_number(fields.next());
    let total = parse_number(fields.next());
    let total_estimate = parse_number(fields.next());
    let speed = parse_number(fields.next());
    let eta = parse_number(fields.next()).map(|value| value.max(0.0) as u64);
    let total = total.or(total_estimate);
    let percent = match (downloaded, total) {
        (Some(current), Some(total)) if total > 0.0 => Some((current / total * 100.0).clamp(0.0, 100.0)),
        _ => None,
    };
    Some(ParsedProgress { percent, speed, eta })
}

fn parse_number(value: Option<&str>) -> Option<f64> {
    let value = value?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("NA") || value.eq_ignore_ascii_case("none") { None } else { value.parse().ok() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_machine_progress() {
        let parsed = parse_progress("ytharbor-progress:50|200|NA|1024|8").unwrap();
        assert_eq!(parsed.percent, Some(25.0));
        assert_eq!(parsed.speed, Some(1024.0));
        assert_eq!(parsed.eta, Some(8));
    }
}
