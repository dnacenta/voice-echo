mod ansi;
mod checks;
mod prompts;
mod writer;

use std::io::IsTerminal;

use rand::Rng;

use writer::SetupValues;

/// Entry point for `trinity-echo --setup`.
pub fn run() {
    if !std::io::stdin().is_terminal() {
        eprintln!("Error: --setup requires an interactive terminal");
        std::process::exit(1);
    }

    println!();
    println!("  {}", ansi::bold("trinity-echo setup"));
    println!("  {}", ansi::dim("Interactive configuration wizard"));

    // Prerequisite checks
    if !checks::run_checks() {
        std::process::exit(1);
    }

    // Twilio
    println!("\n  {} Twilio Configuration", ansi::bold(">>"));
    let twilio_account_sid = prompts::ask_secret("Account SID");
    let twilio_auth_token = prompts::ask_secret("Auth Token");
    let twilio_phone_number = loop {
        let num = prompts::ask("Phone Number (E.164)", None);
        if validate_e164(&num) {
            break num;
        }
        println!("  {} Invalid E.164 format (expected: +<digits>)", ansi::red("!"));
    };

    // Groq
    println!("\n  {} Groq (Whisper STT)", ansi::bold(">>"));
    let groq_api_key = prompts::ask_secret("API Key");

    // ElevenLabs
    println!("\n  {} ElevenLabs (TTS)", ansi::bold(">>"));
    let elevenlabs_api_key = prompts::ask_secret("API Key");
    let elevenlabs_voice_id =
        prompts::ask("Voice ID", Some("EST9Ui6982FZPSi7gCHi"));

    // Server
    println!("\n  {} Server", ansi::bold(">>"));
    let external_url = prompts::ask("External URL", None);

    // Generate API token
    let api_token = generate_hex_token(32);
    println!(
        "\n  {} Generated TRINITY_API_TOKEN",
        ansi::green("\u{2713}")
    );

    // Write config files
    let values = SetupValues {
        twilio_account_sid,
        twilio_auth_token,
        twilio_phone_number,
        groq_api_key,
        elevenlabs_api_key,
        elevenlabs_voice_id,
        external_url: external_url.clone(),
        api_token,
    };

    writer::write_config(&values);

    // Optional system installation
    println!("\n  {} System installation (optional)", ansi::bold(">>"));

    if prompts::confirm("Copy binary to /usr/local/bin/?") {
        writer::install_binary();
    }

    if prompts::confirm("Install systemd service?") {
        writer::install_systemd();
    }

    if prompts::confirm("Generate nginx config?") {
        writer::install_nginx(&external_url);
    }

    // Done
    println!("\n  {} Setup complete!", ansi::green("\u{2713}"));
    println!();
    println!("  Next steps:");
    println!("    1. Review ~/.trinity-echo/config.toml");
    println!("    2. Run: trinity-echo");
    println!(
        "    3. Set Twilio voice webhook to {}/twilio/voice",
        external_url
    );
    println!();
}

/// Basic E.164 validation: starts with +, followed by digits only, 8-15 total chars.
fn validate_e164(s: &str) -> bool {
    if !s.starts_with('+') || s.len() < 8 || s.len() > 16 {
        return false;
    }
    s[1..].chars().all(|c| c.is_ascii_digit())
}

/// Generate a hex token of `byte_len` random bytes (output is 2x byte_len chars).
fn generate_hex_token(byte_len: usize) -> String {
    let mut rng = rand::thread_rng();
    let bytes: Vec<u8> = (0..byte_len).map(|_| rng.gen()).collect();
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
