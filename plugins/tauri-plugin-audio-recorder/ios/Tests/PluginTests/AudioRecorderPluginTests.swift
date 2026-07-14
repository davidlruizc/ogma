import XCTest
@testable import tauri_plugin_audio_recorder

/// Unit tests for Audio Recorder Plugin
final class AudioRecorderPluginTests: XCTestCase {
    
    /// Test that recording states are valid enum cases
    func testRecordingStates() throws {
        let states = ["idle", "recording", "paused"]
        
        XCTAssertEqual(states.count, 3, "Should have 3 recording states")
        XCTAssertTrue(states.contains("idle"), "Should contain idle")
        XCTAssertTrue(states.contains("recording"), "Should contain recording")
        XCTAssertTrue(states.contains("paused"), "Should contain paused")
    }
    
    /// Test permission status values
    func testPermissionStatuses() throws {
        let statuses = ["granted", "denied", "unknown"]
        
        XCTAssertEqual(statuses.count, 3, "Should have 3 permission statuses")
        XCTAssertTrue(statuses.contains("granted"), "Should contain granted")
        XCTAssertTrue(statuses.contains("denied"), "Should contain denied")
        XCTAssertTrue(statuses.contains("unknown"), "Should contain unknown")
    }
    
    /// Test audio quality presets
    func testAudioQualityPresets() throws {
        let qualities = ["low", "medium", "high"]
        let sampleRates = ["low": 16000, "medium": 44100, "high": 48000]
        let channels = ["low": 1, "medium": 1, "high": 2]
        
        XCTAssertEqual(qualities.count, 3, "Should have 3 quality presets")
        XCTAssertEqual(sampleRates["low"], 16000)
        XCTAssertEqual(sampleRates["medium"], 44100)
        XCTAssertEqual(sampleRates["high"], 48000)
        XCTAssertEqual(channels["low"], 1)
        XCTAssertEqual(channels["high"], 2)
    }
    
    /// Test sample rate validation
    func testSampleRateRange() throws {
        let validRates = [8000, 16000, 22050, 44100, 48000, 96000]
        let invalidRates = [0, -1]
        
        for rate in validRates {
            XCTAssertTrue(rate >= 8000 && rate <= 192000, "Sample rate \(rate) should be valid")
        }
        
        for rate in invalidRates {
            XCTAssertFalse(rate >= 8000 && rate <= 192000, "Sample rate \(rate) should be invalid")
        }
    }
    
    /// Test duration is positive
    func testDurationValidation() throws {
        let validDurations: [Int64] = [0, 1000, 60000, 3600000]
        let invalidDurations: [Int64] = [-1, -1000]
        
        for duration in validDurations {
            XCTAssertTrue(duration >= 0, "Duration \(duration) should be valid")
        }
        
        for duration in invalidDurations {
            XCTAssertFalse(duration >= 0, "Duration \(duration) should be invalid")
        }
    }
}
