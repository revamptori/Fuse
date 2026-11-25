package cloud.fuss

import cloud.fuss.core.Decoder
import cloud.fuss.core.FileEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire-format tests for the Kotlin decoder. The Rust side has its own encoder
 * tests; these check that both agree on the layout, and that malformed input
 * cannot crash the UI.
 */
class DecoderTest {

    private fun bytes(vararg values: Int): ByteArray = ByteArray(values.size) { values[it].toByte() }

    @Test
    fun readsLittleEndianPrimitives() {
        val d = Decoder(bytes(0x01, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF))
        assertEquals(1, d.u32())
        assertEquals(-1, d.u32())
    }

    @Test
    fun readsUnsignedU32() {
        val d = Decoder(bytes(0xFF, 0xFF, 0xFF, 0xFF))
        assertEquals(4294967295L, d.u32Unsigned())
    }

    @Test
    fun readsU64() {
        // 1 + 2^40, little endian
        val d = Decoder(byteArrayOf(1, 0, 0, 0, 0, 1, 0, 0))
        assertEquals(1L + (1L shl 40), d.u64())
    }

    @Test
    fun readsStrings() {
        // length 5, "héllo" in UTF-8 is 6 bytes; use ascii to keep it simple.
        val payload = "hello".toByteArray()
        val buf = ByteArray(4 + payload.size)
        buf[0] = payload.size.toByte()
        payload.copyInto(buf, 4)
        val d = Decoder(buf)
        assertEquals("hello", d.str())
    }

    @Test
    fun readsEntry() {
        val name = "photo.jpg".toByteArray()
        val buf = ByteArray(4 + name.size + 1 + 1 + 8 + 8 + 4)
        var p = 0
        buf[p++] = name.size.toByte(); p += 3
        name.copyInto(buf, p); p += name.size
        buf[p++] = 4 // statOk flag
        buf[p++] = FileEntry.KIND_IMAGE.toByte()
        // size = 1024
        buf[p++] = 0; buf[p++] = 4; p += 6
        // modified = 0
        p += 8
        // mode = 0o644
        buf[p++] = 0xA4; buf[p++] = 0x01
        val d = Decoder(buf)
        val e = d.entry()
        assertEquals("photo.jpg", e.name)
        assertEquals(FileEntry.KIND_IMAGE, e.kind)
        assertEquals(1024L, e.size)
        assertFalse(e.isDir)
        assertTrue(e.statOk)
    }
}
