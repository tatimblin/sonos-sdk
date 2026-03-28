//! Live test of group lifecycle methods using standalone speakers
//!
//! Tests add_speaker, remove_speaker, dissolve, create_group, join_group, leave_group
//! using non-bonded standalone speakers (Bathroom and Bedroom Connect:Amps).
//!
//! Run with: cargo run -p sonos-sdk --example group_lifecycle_test

use sonos_sdk::{SdkError, SonosSystem};
use std::thread;
use std::time::Duration;

fn main() -> Result<(), SdkError> {
    println!("=== Group Lifecycle Live Test ===\n");

    let system = SonosSystem::new()?;
    let speaker_names = system.speaker_names();
    println!(
        "Discovered {} speakers: {}\n",
        speaker_names.len(),
        speaker_names.join(", ")
    );

    // Bootstrap topology by watching group_membership on any speaker
    println!("Bootstrapping topology...");
    let _topology_handle = system
        .speaker(&speaker_names[0])
        .and_then(|s| s.group_membership.watch().ok());

    // Wait for topology event
    for i in 1..=10 {
        let groups = system.groups();
        if !groups.is_empty() {
            println!(
                "Topology populated: {} groups (waited {}ms)\n",
                groups.len(),
                i * 500
            );
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    // Print current topology
    print_topology(&system);

    // Find two standalone speakers that are NOT home theater devices (no Playbar/Beam/Arc)
    // Home theater primaries with bonded surrounds may have AddMember restrictions
    let groups = system.groups();
    let standalone_groups: Vec<_> = groups
        .iter()
        .filter(|g| {
            if g.member_count() != 1 {
                return false;
            }
            // Skip home theater speakers (Playbar, Beam, Arc, Sub)
            if let Some(coord) = g.coordinator() {
                let model = coord.model_name.to_lowercase();
                !model.contains("playbar")
                    && !model.contains("beam")
                    && !model.contains("arc")
                    && !model.contains("sub")
            } else {
                false
            }
        })
        .collect();

    println!("Standalone non-HT speakers:");
    for g in &standalone_groups {
        if let Some(c) = g.coordinator() {
            println!("  {} ({}) — {}", c.name, c.id, c.model_name);
        }
    }
    println!();

    if standalone_groups.len() < 2 {
        println!("ERROR: Need at least 2 standalone non-home-theater speakers for this test.");
        println!(
            "Found {} qualifying groups. Ungroup some speakers and retry.",
            standalone_groups.len()
        );
        return Ok(());
    }

    let coordinator_group = &standalone_groups[0];
    let member_group = &standalone_groups[1];

    let coordinator = coordinator_group
        .coordinator()
        .ok_or_else(|| SdkError::InvalidOperation("No coordinator".to_string()))?;
    let member = member_group
        .coordinator()
        .ok_or_else(|| SdkError::InvalidOperation("No member".to_string()))?;

    println!("--- Test Speakers ---");
    println!(
        "Coordinator: {} ({}) at {}",
        coordinator.name, coordinator.id, coordinator.ip
    );
    println!(
        "Member:      {} ({}) at {}",
        member.name, member.id, member.ip
    );

    // Check boot_seq values
    let coord_boot_seq = system.state_manager().get_boot_seq(&coordinator.id);
    let member_boot_seq = system.state_manager().get_boot_seq(&member.id);
    println!("Coordinator boot_seq: {coord_boot_seq:?}");
    println!("Member boot_seq:      {member_boot_seq:?}");
    println!();

    // ===== TEST 1: add_speaker =====
    println!("=== TEST 1: group.add_speaker(&member) ===");
    let coord_group = system
        .group_for_speaker(&coordinator.id)
        .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;

    match coord_group.add_speaker(&member) {
        Ok(()) => println!("OK — add_speaker succeeded"),
        Err(e) => {
            println!("FAILED: {e}");
            println!("\nSkipping remaining tests.");
            return Ok(());
        }
    }

    // Wait and poll for topology update
    println!("\nPolling topology for changes...");
    let mut saw_change = false;
    for i in 1..=40 {
        thread::sleep(Duration::from_millis(500));
        let updated_groups = system.groups();
        // Check if coordinator's group now has >1 member
        for g in &updated_groups {
            if g.coordinator_id == coordinator.id && g.member_count() > 1 {
                println!(
                    "Topology updated at {}ms — coordinator's group now has {} members",
                    i * 500,
                    g.member_count()
                );
                saw_change = true;
                break;
            }
        }
        if saw_change {
            break;
        }
        if i % 10 == 0 {
            println!("  ...still waiting ({}s)", i / 2);
        }
    }
    if !saw_change {
        println!("No topology change detected after 20s");
    }
    print_topology(&system);

    // ===== TEST 2: remove_speaker =====
    println!("=== TEST 2: group.remove_speaker(&member) ===");
    // Re-fetch the group to get updated membership
    let coord_group = system
        .group_for_speaker(&coordinator.id)
        .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;

    match coord_group.remove_speaker(&member) {
        Ok(()) => println!("OK — RemoveMember succeeded"),
        Err(e) => println!("FAILED: {e}"),
    }

    // Wait for topology update
    println!("\nPolling topology for changes...");
    saw_change = false;
    for i in 1..=40 {
        thread::sleep(Duration::from_millis(500));
        let updated_groups = system.groups();
        for g in &updated_groups {
            if g.coordinator_id == coordinator.id && g.member_count() == 1 {
                println!(
                    "Topology updated at {}ms — coordinator's group back to {} member",
                    i * 500,
                    g.member_count()
                );
                saw_change = true;
                break;
            }
        }
        if saw_change {
            break;
        }
        if i % 10 == 0 {
            println!("  ...still waiting ({}s)", i / 2);
        }
    }
    if !saw_change {
        println!("No topology change detected after 20s");
    }
    print_topology(&system);

    // ===== TEST 3: speaker.join_group =====
    println!("=== TEST 3: member.join_group(&coord_group) ===");
    let coord_group = system
        .group_for_speaker(&coordinator.id)
        .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;

    match member.join_group(&coord_group) {
        Ok(()) => println!("OK — join_group succeeded"),
        Err(e) => println!("FAILED: {e}"),
    }

    wait_for_topology_change(&system, &coordinator.id, |count| count > 1, "join");
    print_topology(&system);

    // ===== TEST 4: speaker.leave_group =====
    println!("=== TEST 4: member.leave_group() ===");
    match member.leave_group() {
        Ok(resp) => println!(
            "OK — delegated_group_coordinator_id: '{}', new_group_id: '{}'",
            resp.delegated_group_coordinator_id, resp.new_group_id
        ),
        Err(e) => println!("FAILED: {e}"),
    }

    wait_for_topology_change(&system, &coordinator.id, |count| count == 1, "leave");
    print_topology(&system);

    // ===== TEST 5: system.create_group =====
    println!("=== TEST 5: system.create_group(&coordinator, &[&member]) ===");
    match system.create_group(&coordinator, &[&member]) {
        Ok(result) => {
            println!(
                "OK — {} succeeded, {} failed",
                result.succeeded.len(),
                result.failed.len()
            );
            for (id, err) in &result.failed {
                println!("  FAILED {id}: {err}");
            }
        }
        Err(e) => println!("FAILED: {e}"),
    }

    wait_for_topology_change(&system, &coordinator.id, |count| count > 1, "create_group");
    print_topology(&system);

    // ===== TEST 6: group.dissolve =====
    println!("=== TEST 6: group.dissolve() ===");
    let coord_group = system
        .group_for_speaker(&coordinator.id)
        .ok_or_else(|| SdkError::SpeakerNotFound(coordinator.id.as_str().to_string()))?;

    let result = coord_group.dissolve();
    println!(
        "OK — {} succeeded, {} failed",
        result.succeeded.len(),
        result.failed.len()
    );
    for (id, err) in &result.failed {
        println!("  FAILED {id}: {err}");
    }

    wait_for_topology_change(&system, &coordinator.id, |count| count == 1, "dissolve");
    print_topology(&system);

    println!("\n=== All tests complete ===");

    Ok(())
}

fn print_topology(system: &SonosSystem) {
    let groups = system.groups();
    println!("--- Current Topology ({} groups) ---", groups.len());
    for group in &groups {
        let coord_name = group
            .coordinator()
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "?".to_string());
        let member_names: Vec<String> = group.members().iter().map(|s| s.name.clone()).collect();
        println!(
            "  [{}] {} — {} members: [{}]",
            &group.id.as_str()[..8.min(group.id.as_str().len())],
            coord_name,
            group.member_count(),
            member_names.join(", ")
        );
    }
    println!();
}

fn wait_for_topology_change(
    system: &SonosSystem,
    coordinator_id: &sonos_state::SpeakerId,
    condition: fn(usize) -> bool,
    label: &str,
) {
    println!("\nPolling topology for {label} change...");
    for i in 1..=40 {
        thread::sleep(Duration::from_millis(500));
        let groups = system.groups();
        for g in &groups {
            if g.coordinator_id == *coordinator_id && condition(g.member_count()) {
                println!("Topology updated at {}ms", i * 500);
                return;
            }
        }
        if i % 10 == 0 {
            println!("  ...still waiting ({}s)", i / 2);
        }
    }
    println!("No topology change detected after 20s for {label}");
}
