package cloud.fuss

import cloud.fuss.ui.Format
import org.junit.Assert.assertEquals
import org.junit.Test

class FormatTest {
    @Test
    fun sizes() {
        assertEquals("0 B", Format.size(0))
        assertEquals("999 B", Format.size(999))
        assertEquals("1.00 KB", Format.size(1024))
        assertEquals("1.50 MB", Format.size(1024L * 1024 * 3 / 2))
        assertEquals("1.00 GB", Format.size(1024L * 1024 * 1024))
        assertEquals("1.00 TB", Format.size(1024L * 1024 * 1024 * 1024))
        // Unknown sizes report a dash rather than a misleading number.
        assertEquals("-", Format.size(-1))
        // Three significant figures change below/above 100 in a unit.
        assertEquals("9.99 KB", Format.size(10229))
        assertEquals("512 KB", Format.size(512L * 1024))
    }

    @Test
    fun permissions() {
        assertEquals("drwxr-xr-x", Format.permissions(0x41ED))
        assertEquals("-rw-r--r--", Format.permissions(0x81A4))
        assertEquals("lrwxrwxrwx", Format.permissions(0xA1FF))
    }
}
