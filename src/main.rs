use console::{style, Term};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    terminal,
};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
struct OpenAIResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Choice {
    index: i32,
    message: Message,
    finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

async fn get_suggested_commit_messages(diff: &str) -> Result<Vec<String>, reqwest::Error> {
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not found");
    let client = Client::new();
    let prompt = format!("Given the following git diff, suggest a single commit message of no more than 50 characters:\n\n```\n{}\n```\n\nOutput only the commit message as it would be passed to `git commit`. Do not include an explanation, and do not wrap the message in quotation marks.", diff);

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&serde_json::json!({
            "model": "gpt-3.5-turbo",
            "messages": [
                {"role": "system", "content": "You are a helpful assistant."},
                {"role": "user", "content": prompt}
            ],
            "n": 5
        }))
        .send()
        .await?
        .json::<OpenAIResponse>()
        .await?;

    let messages = response
        .choices
        .into_iter()
        .map(|choice| {
            let s = choice.message.content.trim();
            let result = if s.starts_with('"') && s.ends_with('"') {
                &s[1..s.len() - 1]
            } else {
                &s
            };
            result.to_string()
        })
        .collect();
    Ok(messages)
}

fn select_commit_message(commit_messages: Vec<String>) -> Option<String> {
    let term = Term::stdout();
    let mut index: usize = 0;

    loop {
        println!("Select a commit message:");
        for (i, msg) in commit_messages.iter().enumerate() {
            if i == index {
                println!("{} {}", style(">").bold().green(), msg);
            } else {
                println!("  {}", msg);
            }
        }

        terminal::enable_raw_mode().unwrap();
        let key_event = event::read().unwrap();
        terminal::disable_raw_mode().unwrap();
        term.clear_last_lines(commit_messages.len() + 1).unwrap();

        match key_event {
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                if index > 0 {
                    index -= 1;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                ..
            }) => {
                if index < commit_messages.len() - 1 {
                    index += 1;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => return None,
            _ => {}
        }
    }

    Some(commit_messages[index].clone())
}

#[tokio::main]
async fn main() {
    let git_diff_output = Command::new("git")
        .args(&["--no-pager", "diff", "--staged"])
        .output()
        .expect("Failed to execute git diff command");

    if !git_diff_output.status.success() {
        eprintln!(
            "Error executing git diff command: {:?}",
            git_diff_output.status
        );
        return;
    }

    let git_diff =
        String::from_utf8(git_diff_output.stdout).expect("Invalid UTF-8 in git diff output");

    if git_diff.trim().is_empty() {
        eprintln!("no changes added to commit (use \"git add\" and/or \"git commit -a\")");
        return;
    }

    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(100));
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {wide_msg}").unwrap());
    pb.set_message("Fetching suggested commit messages...");

    let commit_messages_result = get_suggested_commit_messages(&git_diff).await;

    pb.finish_and_clear();

    match commit_messages_result {
        Ok(commit_messages) => {
            let mut options = vec!["Enter a custom message...".to_string()];
            options.extend(commit_messages.iter().cloned());
            if let Some(selected_message) = select_commit_message(options) {
                if selected_message == "Enter a custom message..." {
                    Command::new("git")
                        .args(["commit"])
                        .spawn()
                        .unwrap()
                        .wait()
                        .unwrap();
                } else {
                    Command::new("git")
                        .args(["commit", "-m", &selected_message])
                        .spawn()
                        .unwrap()
                        .wait()
                        .unwrap();
                }
            }
        }
        Err(e) => eprintln!("Error: {}", e),
    }
}
