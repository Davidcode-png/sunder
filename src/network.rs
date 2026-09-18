use serde_json::Value;

pub async fn fetch_model_id(base_url: &str) -> Result<String, String> {
    let model_url = format!("{base_url}/models");
    let response = reqwest::get(model_url).await.map_err(|e| e.to_string())?;

    let json_val: Value = response.json().await.map_err(|e| e.to_string())?;

    if let Some(models) = json_val["data"].as_array() {
        for model in models {
            if let Some(name) = model["id"].as_str() {
                return Ok(name.to_string());
            }
        }
    }

    Err("no models available".to_string())
}
