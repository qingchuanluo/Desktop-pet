use rand::Rng;

pub fn pick_event_phrase(
    texts: Option<&crate::character::CharacterTexts>,
    event: &str,
) -> Option<String> {
    let list = texts
        .and_then(|t| t.event_phrases.as_ref())
        .and_then(|m| m.get(event))
        .filter(|v| !v.is_empty());
    if let Some(v) = list {
        let mut rng = rand::thread_rng();
        let idx = rng.gen_range(0..v.len());
        return Some(v[idx].clone());
    }

    match event {
        "pet_clicked" => {
            let v = texts
                .and_then(|t| t.pet_clicked_phrases.as_ref())
                .filter(|v| !v.is_empty())?;
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..v.len());
            Some(v[idx].clone())
        }
        "feed" => {
            let v = texts
                .and_then(|t| t.feed_phrases.as_ref())
                .filter(|v| !v.is_empty())?;
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..v.len());
            Some(v[idx].clone())
        }
        _ => None,
    }
}
