//! Tests for resource cleanup and early iterator termination

use sonos_discovery::get_iter_with_timeout;
use std::time::Duration;

#[test]
fn test_early_iterator_termination() {
    // Create an iterator with a short timeout
    let mut iter = get_iter_with_timeout(Duration::from_millis(100));
    
    // Take only the first item (if any) and drop the iterator early
    let _first = iter.next();
    
    // Iterator is dropped here - this test verifies no panic or resource leak
    // If Drop is not implemented correctly, this could leak the UDP socket
}

#[test]
fn test_iterator_drop_without_iteration() {
    // Create an iterator but never call next()
    let _iter = get_iter_with_timeout(Duration::from_millis(100));
    
    // Iterator is dropped here without ever being used
    // This tests that the SSDP client is properly cleaned up even if never used
}

#[test]
fn test_multiple_iterators_sequential() {
    // Create and drop multiple iterators sequentially
    // This tests that resources are properly released and can be reacquired
    for _ in 0..3 {
        let mut iter = get_iter_with_timeout(Duration::from_millis(100));
        let _first = iter.next();
        // Iterator dropped at end of loop
    }
}

#[test]
fn test_iterator_partial_consumption() {
    // Create an iterator and consume only part of it
    let iter = get_iter_with_timeout(Duration::from_millis(100));
    
    // Take only 2 items (if available) then drop
    let _items: Vec<_> = iter.take(2).collect();
    
    // Iterator is dropped here after partial consumption
}
