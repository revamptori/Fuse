package cloud.fuss

import cloud.fuss.core.Decoder
import cloud.fuss.core.FileEntry
import cloud.fuss.core.Wire
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Wire-format tests for the Kotlin decoder. The Rust encoder has its own tests;
 * these pin the layout from the consumer side, and check that broken input is
 * reported instead of crashing.
 *
 * Layout (little endian): u8/u32/u64 primitives, strings are u32 length + UTF-8,
 * an entry is name, flags byte (1 dir, 2 symlink, 4 statok), kind byte, u64 size,
 * i64 mtime, u32 mode.
 */
class DecoderTest {

    private fun le32(v: Int) = byteArrayOf(
        (v and 0xFF).toByte(), ((v shr 8) and 0xFF).toByte(),
        ((v shr 16) and 0xFF).toByte(), ((v shr 24) and 0xFF).toByte(),
    )

    private fun le64(v: Long) = (0 until 8).map { ((v shr (8 * it)) and 0xFF).toByte() }.toByteArray()

    private fun str(s: String): ByteArray = le32(s.toByteArray().size) + s.toByteArray()

    @Test
    fun readsLittleEndianPrimitives() {
        val d = Decoder(le32(1) + le32(-1))
        assertEquals(1, d.u32())
        assertEquals(-1, d.u32())
    }

    @Test
    fun readsUnsignedU32() {
        assertEquals(4294967295L, Decoder(le32(-1)).u32Unsigned())
    }

    @Test
    fun readsU64() {
        assertEquals(1L + (1L shl 40), Decoder(le64(1L + (1L shl 40))).u64())
    }

    @Test
    fun readsStrings() {
        assertEquals("héllo", Decoder(str("héllo")).str())
    }

    @Test
    fun readsEntry() {
        val payload = str("photo.jpg") +
            byteArrayOf(4) +               // statOk
            byteArrayOf(FileEntry.KIND_IMAGE.toByte()) +
            le64(1024) +
            le64(1_700_000_000) +
            le32(0x1A4)
        val e = Decoder(payload).entry()
        assertEquals("photo.jpg", e.name)
        assertEquals(FileEntry.KIND_IMAGE, e.kind)
        assertEquals(1024L, e.size)
        assertEquals(1_700_000_000L, e.modified)
        assertFalse(e.isDir)
        assertFalse(e.isSymlink)
        assertTrue(e.statOk)
    }

    @Test
    fun entryFlags() {
        val payload = str("dir") + byteArrayOf(5) + byteArrayOf(0) + le64(0) + le64(0) + le32(0x41ED)
        val e = Decoder(payload).entry()
        assertTrue(e.isDir)
        assertTrue(e.statOk)
        assertFalse(e.isSymlink)
    }

    @Test
    fun wireDecodesSuccess() {
        val inner = le32(7)
        val msg = byteArrayOf(0) + le32(inner.size) + inner
        assertEquals(7, Wire.decode(msg) { it.u32() })
    }

    @Test
    fun wireThrowsEngineErrors() {
        val msg = byteArrayOf(1) + str("permission_denied") + str("EACCES")
        val ex = assertThrows(cloud.fuss.core.FussException::class.java) {
            Wire.decode(msg) { it.u32() }
        }
        assertEquals("permission_denied", ex.code)
        assertEquals("EACCES", ex.message?.substringBefore(" (") ?: "EACCES")
        assertEquals("Permission denied", ex.userMessage())
    }

    @Test
    fun wireRejectsMissingPayload() {
        val ex = assertThrows(cloud.fuss.core.FussException::class.java) {
            Wire.decode(null) { it.u32() }
        }
        assertNull(ex.path.ifEmpty { null })
    }
}
