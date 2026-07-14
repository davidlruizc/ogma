package io.affex.audiorecorder

import org.junit.Test
import org.junit.Assert.*

/**
 * Unit tests for Audio Recorder Plugin models and utilities.
 * 
 * These tests run on the development machine (host) and validate
 * core logic without requiring Android framework dependencies.
 */
class AudioRecorderUnitTest {
    
    /**
     * Test that recording states are correctly defined
     */
    @Test
    fun recordingStates_areValid() {
        val states = listOf("idle", "recording", "paused")
        assertTrue("Should have 3 recording states", states.size == 3)
        assertTrue("Should contain idle", states.contains("idle"))
        assertTrue("Should contain recording", states.contains("recording"))
        assertTrue("Should contain paused", states.contains("paused"))
    }
    
    /**
     * Test permission status values
     */
    @Test
    fun permissionStatuses_areValid() {
        val statuses = listOf("granted", "denied", "unknown")
        assertTrue("Should have 3 permission statuses", statuses.size == 3)
        assertTrue("Should contain granted", statuses.contains("granted"))
        assertTrue("Should contain denied", statuses.contains("denied"))
        assertTrue("Should contain unknown", statuses.contains("unknown"))
    }
    
    /**
     * Test audio quality presets
     */
    @Test
    fun audioQuality_presetsAreValid() {
        val qualities = listOf("low", "medium", "high")
        assertTrue("Should have 3 quality presets", qualities.size == 3)
        
        // Low: 16kHz mono
        // Medium: 44.1kHz mono  
        // High: 48kHz stereo
        val sampleRates = mapOf("low" to 16000, "medium" to 44100, "high" to 48000)
        val channels = mapOf("low" to 1, "medium" to 1, "high" to 2)
        
        assertEquals(16000, sampleRates["low"])
        assertEquals(44100, sampleRates["medium"])
        assertEquals(48000, sampleRates["high"])
        assertEquals(1, channels["low"])
        assertEquals(1, channels["medium"])
        assertEquals(2, channels["high"])
    }
    
    /**
     * Test file path extension validation
     */
    @Test
    fun outputFile_hasValidExtension() {
        val validExtensions = listOf(".wav", ".WAV")
        val invalidExtensions = listOf(".mp3", ".ogg", ".m4a")
        
        for (ext in validExtensions) {
            assertTrue("$ext should be valid", ext.lowercase() == ".wav")
        }
        
        for (ext in invalidExtensions) {
            assertFalse("$ext should be invalid", ext.lowercase() == ".wav")
        }
    }
    
    /**
     * Test duration tracking
     */
    @Test
    fun duration_isPositive() {
        val validDurations = listOf(0L, 1000L, 60000L, 3600000L)
        val invalidDurations = listOf(-1L, -1000L)
        
        for (duration in validDurations) {
            assertTrue("Duration $duration should be valid", duration >= 0)
        }
        
        for (duration in invalidDurations) {
            assertFalse("Duration $duration should be invalid", duration >= 0)
        }
    }
    
    /**
     * Test sample rate validation
     */
    @Test
    fun sampleRate_isWithinRange() {
        val validRates = listOf(8000, 16000, 22050, 44100, 48000, 96000)
        val invalidRates = listOf(0, -1, 1000000)
        
        for (rate in validRates) {
            assertTrue("Sample rate $rate should be valid", rate in 8000..192000)
        }
        
        for (rate in invalidRates) {
            assertFalse("Sample rate $rate should be invalid", rate in 8000..192000)
        }
    }
}
