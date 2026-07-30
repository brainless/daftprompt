use llm_sdk::{
    groq::{types::GroqResponseFormat, GroqClient},
    tools::{Tool, ToolChoice},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

const MODEL: &str = "openai/gpt-oss-20b";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
struct LookupArgs {
    document_id: String,
    heading: String,
}

#[derive(Debug, Deserialize)]
struct JsonResult {
    source_type: String,
    result_id: u32,
    relevant: bool,
}

fn usage(response: &llm_sdk::groq::types::GroqChatCompletionResponse) -> String {
    response.usage.as_ref().map_or_else(
        || "unavailable".into(),
        |usage| {
            format!(
                "prompt={},completion={},total={}",
                usage.prompt_tokens, usage.completion_tokens, usage.total_tokens
            )
        },
    )
}

async fn completion(client: &GroqClient) -> anyhow::Result<bool> {
    let started = Instant::now();
    let response = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client
            .message_builder()
            .model(MODEL)
            .max_tokens(96)
            .temperature(0.0)
            .reasoning_effort("low")
            .system_message("Answer with exactly the requested token and nothing else.")
            .user_message("Return exactly: KEYSTONE_OK")
            .send(),
    )
    .await??;
    let choice = response
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("minimal completion returned no choice"))?;
    let output = choice.message.content.trim();
    let passed = output == "KEYSTONE_OK";
    println!(
        "test=minimal pass={passed} model={} latency_ms={} finish_reason={} usage={} output={output:?}",
        response.model,
        started.elapsed().as_millis(),
        choice.finish_reason.as_deref().unwrap_or("unavailable"),
        usage(&response),
    );
    Ok(passed)
}

async fn tool_call(client: &GroqClient) -> anyhow::Result<bool> {
    let tool = Tool::from_type::<LookupArgs>()
        .name("lookup_heading")
        .description("Read one heading from a known document")
        .build();
    let expected = LookupArgs {
        document_id: "LOG-019".into(),
        heading: "Testing".into(),
    };
    let started = Instant::now();
    let response = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client
            .message_builder()
            .model(MODEL)
            .max_tokens(128)
            .temperature(0.0)
            .reasoning_effort("low")
            .system_message("Call the supplied read-only function once. Do not answer in prose.")
            .user_message(
                "Look up the Testing heading in document LOG-019. Use the exact identifiers given.",
            )
            .tool(tool)
            .tool_choice(ToolChoice::Specific {
                name: "lookup_heading".into(),
            })
            .send(),
    )
    .await??;
    let choice = response
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("tool request returned no choice"))?;
    let calls = response.tool_calls().unwrap_or_default();
    let parsed = calls
        .first()
        .and_then(|call| call.parse_arguments::<LookupArgs>().ok());
    let passed = calls.len() == 1
        && calls[0].name() == "lookup_heading"
        && parsed.as_ref() == Some(&expected);
    println!(
        "test=tool_call pass={passed} model={} latency_ms={} finish_reason={} usage={} call_count={} name={:?} args={}",
        response.model,
        started.elapsed().as_millis(),
        choice.finish_reason.as_deref().unwrap_or("unavailable"),
        usage(&response),
        calls.len(),
        calls.first().map(|call| call.name()),
        calls
            .first()
            .map(|call| call.arguments().to_string())
            .unwrap_or_else(|| "null".into()),
    );
    Ok(passed)
}

async fn json_object(client: &GroqClient) -> anyhow::Result<bool> {
    let started = Instant::now();
    let response = tokio::time::timeout(
        REQUEST_TIMEOUT,
        client
            .message_builder()
            .model(MODEL)
            .max_tokens(128)
            .temperature(0.0)
            .reasoning_effort("low")
            .response_format(GroqResponseFormat::json_object())
            .system_message(
                "Return only a JSON object with exactly source_type (string), \
                 result_id (integer), relevant (boolean).",
            )
            .user_message("Represent this result: source type document, result ID 19, relevant yes.")
            .send(),
    )
    .await??;
    let choice = response
        .choices
        .first()
        .ok_or_else(|| anyhow::anyhow!("JSON request returned no choice"))?;
    let output = choice.message.content.trim();
    let parsed = serde_json::from_str::<JsonResult>(output);
    let passed = matches!(
        &parsed,
        Ok(value)
            if value.source_type == "document" && value.result_id == 19 && value.relevant
    );
    let sanitized_output = parsed
        .map(|value| {
            serde_json::json!({
                "source_type": value.source_type,
                "result_id": value.result_id,
                "relevant": value.relevant,
            })
            .to_string()
        })
        .unwrap_or_else(|error| format!("INVALID_JSON({error})"));
    println!(
        "test=json_object pass={passed} model={} latency_ms={} finish_reason={} usage={} output={sanitized_output}",
        response.model,
        started.elapsed().as_millis(),
        choice.finish_reason.as_deref().unwrap_or("unavailable"),
        usage(&response),
    );
    Ok(passed)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // dotenv() preserves an already-exported GROQ_API_KEY, so CI/shell configuration wins.
    let _ = dotenvy::dotenv();
    let api_key = std::env::var("GROQ_API_KEY")
        .map_err(|_| anyhow::anyhow!("GROQ_API_KEY is unavailable; no requests were sent"))?;
    if api_key.trim().is_empty() {
        anyhow::bail!("GROQ_API_KEY is empty; no requests were sent");
    }

    let client = GroqClient::new(api_key)?;
    let minimal = completion(&client).await?;
    let tool = tool_call(&client).await?;
    let json = json_object(&client).await?;
    let passed = [minimal, tool, json].into_iter().filter(|passed| *passed).count();
    println!("summary pass={passed} fail={}", 3 - passed);
    anyhow::ensure!(passed == 3, "one or more smoke tests failed");
    Ok(())
}
