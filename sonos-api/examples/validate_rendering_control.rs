//! Validates the new RenderingControl operations against a real Sonos speaker.
//! Run with: cargo run -p sonos-api --example validate_rendering_control

use sonos_api::SonosClient;
use sonos_api::services::rendering_control::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Discover speakers
    println!("Discovering Sonos speakers...");
    let devices = sonos_discovery::get();
    if devices.is_empty() {
        println!("No Sonos devices found! Make sure you're on the same network.");
        return Ok(());
    }

    for (i, device) in devices.iter().enumerate() {
        println!("  [{}] {} ({}) at {}", i, device.name, device.model_name, device.ip_address);
    }

    let device = &devices[0];
    let ip = device.ip_address.to_string();
    println!("\nUsing: {} at {}\n", device.name, ip);

    let client = SonosClient::new();

    // 2. Test existing operation (GetVolume) as baseline
    println!("=== Baseline: GetVolume ===");
    let op = get_volume_operation("Master".to_string()).build()?;
    let response = client.execute_enhanced(&ip, op)?;
    println!("  Volume: {}", response.current_volume);

    // 3. Test GetMute
    println!("\n=== GetMute ===");
    let op = get_mute_operation("Master".to_string()).build()?;
    let response = client.execute_enhanced(&ip, op)?;
    println!("  Mute (Master): {}", response.current_mute);

    // 4. Test GetBass
    println!("\n=== GetBass ===");
    let op = get_bass_operation().build()?;
    let response = client.execute_enhanced(&ip, op)?;
    println!("  Bass: {} (range: -10 to +10)", response.current_bass);

    // 5. Test GetTreble
    println!("\n=== GetTreble ===");
    let op = get_treble_operation().build()?;
    let response = client.execute_enhanced(&ip, op)?;
    println!("  Treble: {} (range: -10 to +10)", response.current_treble);

    // 6. Test GetLoudness
    println!("\n=== GetLoudness ===");
    let op = get_loudness_operation("Master".to_string()).build()?;
    let response = client.execute_enhanced(&ip, op)?;
    println!("  Loudness (Master): {}", response.current_loudness);

    // 7. Test SetMute (mute, then unmute)
    println!("\n=== SetMute (round-trip test) ===");
    let get_op = get_mute_operation("Master".to_string()).build()?;
    let original = client.execute_enhanced(&ip, get_op)?.current_mute;
    println!("  Original mute: {}", original);

    let set_op = set_mute_operation("Master".to_string(), !original).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Set mute to: {}", !original);

    let get_op = get_mute_operation("Master".to_string()).build()?;
    let after = client.execute_enhanced(&ip, get_op)?.current_mute;
    println!("  Read back: {}", after);
    assert_eq!(after, !original, "Mute round-trip failed!");

    // Restore original
    let set_op = set_mute_operation("Master".to_string(), original).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Restored to: {}", original);

    // 8. Test SetBass (round-trip)
    println!("\n=== SetBass (round-trip test) ===");
    let get_op = get_bass_operation().build()?;
    let original_bass = client.execute_enhanced(&ip, get_op)?.current_bass;
    println!("  Original bass: {}", original_bass);

    let test_bass: i8 = if original_bass == 0 { 3 } else { 0 };
    let set_op = set_bass_operation(test_bass).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Set bass to: {}", test_bass);

    let get_op = get_bass_operation().build()?;
    let after_bass = client.execute_enhanced(&ip, get_op)?.current_bass;
    println!("  Read back: {}", after_bass);
    assert_eq!(after_bass, test_bass, "Bass round-trip failed!");

    // Restore
    let set_op = set_bass_operation(original_bass).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Restored to: {}", original_bass);

    // 9. Test SetTreble (round-trip)
    println!("\n=== SetTreble (round-trip test) ===");
    let get_op = get_treble_operation().build()?;
    let original_treble = client.execute_enhanced(&ip, get_op)?.current_treble;
    println!("  Original treble: {}", original_treble);

    let test_treble: i8 = if original_treble == 0 { 3 } else { 0 };
    let set_op = set_treble_operation(test_treble).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Set treble to: {}", test_treble);

    let get_op = get_treble_operation().build()?;
    let after_treble = client.execute_enhanced(&ip, get_op)?.current_treble;
    println!("  Read back: {}", after_treble);
    assert_eq!(after_treble, test_treble, "Treble round-trip failed!");

    // Restore
    let set_op = set_treble_operation(original_treble).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Restored to: {}", original_treble);

    // 10. Test SetLoudness (round-trip)
    println!("\n=== SetLoudness (round-trip test) ===");
    let get_op = get_loudness_operation("Master".to_string()).build()?;
    let original_loudness = client.execute_enhanced(&ip, get_op)?.current_loudness;
    println!("  Original loudness: {}", original_loudness);

    let set_op = set_loudness_operation("Master".to_string(), !original_loudness).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Set loudness to: {}", !original_loudness);

    let get_op = get_loudness_operation("Master".to_string()).build()?;
    let after_loudness = client.execute_enhanced(&ip, get_op)?.current_loudness;
    println!("  Read back: {}", after_loudness);
    assert_eq!(after_loudness, !original_loudness, "Loudness round-trip failed!");

    // Restore
    let set_op = set_loudness_operation("Master".to_string(), original_loudness).build()?;
    client.execute_enhanced(&ip, set_op)?;
    println!("  Restored to: {}", original_loudness);

    println!("\n=== ALL VALIDATIONS PASSED ===");
    Ok(())
}
