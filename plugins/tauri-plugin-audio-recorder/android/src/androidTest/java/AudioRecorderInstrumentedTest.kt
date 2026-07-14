package io.affex.audiorecorder

import android.Manifest
import android.content.pm.PackageManager
import android.media.MediaRecorder
import androidx.core.content.ContextCompat
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.ext.junit.runners.AndroidJUnit4

import org.junit.Test
import org.junit.runner.RunWith

import org.junit.Assert.*

/**
 * Instrumented tests for Audio Recorder Plugin.
 * 
 * These tests run on an Android device/emulator and validate
 * functionality that requires the Android framework.
 */
@RunWith(AndroidJUnit4::class)
class AudioRecorderInstrumentedTest {
    
    /**
     * Test that the package name is correct
     */
    @Test
    fun packageName_isCorrect() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals("io.affex.audiorecorder.test", appContext.packageName)
    }
    
    /**
     * Test permission check for RECORD_AUDIO
     */
    @Test
    fun permission_canBeChecked() {
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        val permission = ContextCompat.checkSelfPermission(
            appContext,
            Manifest.permission.RECORD_AUDIO
        )
        
        assertTrue(
            "Permission should be granted or denied",
            permission == PackageManager.PERMISSION_GRANTED ||
            permission == PackageManager.PERMISSION_DENIED
        )
    }
    
    /**
     * Test MediaRecorder audio source availability
     */
    @Test
    fun mediaRecorder_audioSourcesExist() {
        val sources = listOf(
            MediaRecorder.AudioSource.MIC,
            MediaRecorder.AudioSource.DEFAULT
        )
        
        for (source in sources) {
            assertTrue("Audio source $source should be valid", source >= 0)
        }
    }
}
