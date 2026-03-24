use dashmap::DashMap;
use once_cell::sync::Lazy;

static CACHE: Lazy<DashMap<String, String>> = Lazy::new(DashMap::new);

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap_or_default()
});

/// Translate an English word/phrase to Chinese via Google Translate free API.
/// Results are cached in memory so each term is only fetched once.
pub async fn to_chinese(text: &str) -> Option<String> {
    let key = text.to_lowercase();

    if let Some(cached) = CACHE.get(&key) {
        let val = cached.value().clone();
        return if val.is_empty() { None } else { Some(val) };
    }

    let result = fetch_translation(&key).await;

    match &result {
        Some(translation) => CACHE.insert(key, translation.clone()),
        None => CACHE.insert(key, String::new()),
    };

    result
}

async fn fetch_translation(text: &str) -> Option<String> {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=en&tl=zh-CN&dt=t&q={}",
        urlencoding::encode(text)
    );

    let resp = CLIENT.get(&url).send().await.ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;

    // Response format: [[["translated text","original text",...],...],...]]
    let translated = body
        .as_array()?
        .first()?
        .as_array()?
        .iter()
        .filter_map(|segment| segment.as_array()?.first()?.as_str().map(String::from))
        .collect::<Vec<_>>()
        .join("");

    if translated.is_empty() || translated == text {
        None
    } else {
        Some(translated)
    }
}
