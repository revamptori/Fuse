package cloud.fuss

import android.os.Environment
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import cloud.fuss.core.CollisionMode
import cloud.fuss.core.FileEntry
import cloud.fuss.core.FsRepository
import cloud.fuss.core.FussException
import cloud.fuss.core.FussNative
import kotlinx.coroutines.runBlocking
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File

/**
 * Device-side tests: they load `libfuss_core.so` for the device ABI and drive the
 * real engine against real storage. Everything is created under the app's private
 * files directory unless a test explicitly says it uses shared storage.
 *
 * Run with: ./gradlew connectedDebugAndroidTest
 */
@RunWith(AndroidJUnit4::class)
class EngineInstrumentedTest {

    private val repo = FsRepository()
    private lateinit var root: File

    @Before
    fun setUp() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        root = File(context.filesDir, "it-${System.nanoTime()}")
        assertTrue("could not create test dir", root.mkdirs())
    }

    @After
    fun tearDown() {
        root.deleteRecursively()
    }

    private fun write(name: String, bytes: Int): File {
        val f = File(root, name)
        f.parentFile?.mkdirs()
        f.writeBytes(ByteArray(bytes) { (it % 251).toByte() })
        return f
    }

    @Test
    fun nativeLibraryLoadsAndReportsItself() {
        val version = FussNative.version()
        assertTrue("version should not be blank", version.isNotBlank())
        val report = FussNative.selfTest()
        assertTrue("self test mentions a failure: $report", !report.contains("FAIL"))
    }

    @Test
    fun listsRealDirectory() = runBlocking {
        write("a.txt", 10)
        write("b.bin", 20)
        File(root, "dir").mkdirs()
        val page = repo.list(root.absolutePath, showHidden = false, sort = cloud.fuss.core.SortSpec.DEFAULT)
        assertEquals(3, page.entries.size)
        // Directories sort first in the default order.
        assertTrue(page.entries.first().isDir)
        assertTrue(page.entries.any { it.name == "a.txt" && it.size == 10L })
    }

    @Test
    fun hiddenFilesAreFiltered() = runBlocking {
        write("visible.txt", 1)
        write(".hidden", 1)
        val without = repo.list(root.absolutePath, showHidden = false, sort = cloud.fuss.core.SortSpec.DEFAULT)
        val with = repo.list(root.absolutePath, showHidden = true, sort = cloud.fuss.core.SortSpec.DEFAULT)
        assertEquals(1, without.entries.size)
        assertEquals(2, with.entries.size)
    }

    @Test
    fun createRenameCopyMoveDeleteRoundTrip() = runBlocking {
        val created = repo.createFile(root.absolutePath, "one.txt")
        File(created).writeText("hello")
        val renamed = repo.rename(created, "two.txt")
        assertEquals("two.txt", File(renamed).name)
        assertEquals("hello", File(renamed).readText())

        val dir = File(root, "target")
        assertTrue(dir.mkdirs())
        val copied = repo.copy(listOf(renamed), dir.absolutePath, CollisionMode.FAIL)
        assertEquals(0, copied.errorCount.toInt())
        assertEquals("hello", File(dir, "two.txt").readText())

        val moved = repo.move(listOf(File(dir, "two.txt").absolutePath), root.absolutePath, CollisionMode.FAIL)
        assertEquals(0, moved.errorCount.toInt())
        assertFalse(File(dir, "two.txt").exists())

        val deleted = repo.delete(listOf(File(root, "two.txt").absolutePath))
        assertEquals(0, deleted.errorCount.toInt())
        assertFalse(File(root, "two.txt").exists())
    }

    @Test
    fun collisionFailsAndRenameModeSucceeds() = runBlocking {
        write("src.txt", 5)
        val a = File(root, "x.txt").apply { writeText("a") }
        val fail = repo.copy(listOf(a.absolutePath), root.absolutePath, CollisionMode.FAIL)
        assertEquals(1L, fail.errorCount)
        val again = repo.copy(listOf(a.absolutePath), root.absolutePath, CollisionMode.RENAME)
        assertEquals(0, again.errorCount.toInt())
        assertNotNull(again.outputs.firstOrNull())
        assertTrue("renamed copy should not clobber", File(root, "x.txt").readText() == "a")
    }

    @Test
    fun recursiveDeleteRemovesNestedTree() = runBlocking {
        val nested = File(root, "a/b/c").apply { mkdirs() }
        nested.resolve("deep.bin").writeBytes(ByteArray(4096))
        val r = repo.delete(listOf(File(root, "a").absolutePath))
        assertEquals(0, r.errorCount.toInt())
        assertFalse(File(root, "a").exists())
        assertTrue(r.files >= 1L)
    }

    @Test
    fun propertiesReportRealMetadata() = runBlocking {
        val f = write("info.txt", 1234)
        val p = repo.properties(f.absolutePath)
        assertEquals("info.txt", p.name)
        assertEquals(1234L, p.size)
        assertFalse(p.isDir)
        assertTrue(p.modified > 0L)
        assertTrue("mode should carry permission bits", p.mode and 0x1FF != 0)
        assertTrue(p.inode > 0L)
    }

    @Test
    fun missingPathRaisesEngineErrorWithCode() = runBlocking {
        try {
            repo.properties(File(root, "nope").absolutePath)
            fail("expected FussException")
        } catch (e: FussException) {
            assertEquals("not_found", e.code)
            assertTrue(e.userMessage().isNotEmpty())
        }
    }

    @Test
    fun searchFindsNestedMatches() = runBlocking {
        write("deep/inner/needle.txt", 1)
        write("haystack.txt", 1)
        val hits = repo.search(root.absolutePath, "needle", includeHidden = false)
        assertEquals(1, hits.hits.size)
        assertTrue(hits.hits[0].path.endsWith("needle.txt"))
    }

    @Test
    fun cancelledCopyIsReportedAsCancelledOrFinishes() = runBlocking {
        // 48 MiB gives the canceller time to fire mid-transfer on a phone.
        val src = write("big.bin", 48 * 1024 * 1024)
        val dest = File(root, "dest").apply { mkdirs() }
        val handle = FussNative.newCancelToken()
        val canceller = kotlin.concurrent.thread {
            Thread.sleep(10)
            FussNative.cancelToken(handle)
        }
        val r = try {
            decodeCopy(FussNative.copyBatch(arrayOf(src.absolutePath), dest.absolutePath, CollisionMode.FAIL.wire, handle))
        } finally {
            canceller.join(2000)
            FussNative.freeCancelToken(handle)
        }
        if (r.errorCount > 0) {
            assertEquals("cancelled", r.firstErrorCode)
            // An interrupted copy must never claim the file count of a finished one.
            assertTrue("cancelled copy claimed files=${r.files}", r.files == 0L)
        } else {
            assertEquals(48L * 1024 * 1024, File(dest, "big.bin").length())
        }
    }

    /** Decodes the op-result wire format exactly like FsRepository does. */
    private fun decodeCopy(bytes: ByteArray?): cloud.fuss.core.OpResult =
        cloud.fuss.core.Wire.decode(bytes) { d ->
            val files = d.u64()
            val dirs = d.u64()
            val total = d.u64()
            val errors = d.u64()
            val skipped = d.u64()
            val hasError = d.u8() != 0
            val code = if (hasError) d.str() else null
            val message = if (hasError) d.str() else null
            cloud.fuss.core.OpResult(files, dirs, total, errors, skipped, code, message, emptyList())
        }

    @Test
    fun storageAnalysisMeasuresRealTree() = runBlocking {
        write("x/one.bin", 1000)
        write("x/two.bin", 2000)
        write("y/three.bin", 500)
        val sizes = repo.childDirSizes(root.absolutePath)
        val byName = sizes.associateBy { File(it.path).name }
        assertEquals(3000L, byName["x"]?.bytes ?: -1L)
        assertEquals(500L, byName["y"]?.bytes ?: -1L)

        val types = repo.typeStats(root.absolutePath)
        assertTrue(types.isNotEmpty())

        File(root, "empty/nested").mkdirs()
        val empties = repo.emptyDirs(root.absolutePath, 100)
        assertTrue(
            "expected the empty tree to be reported, got $empties",
            empties.any { it.endsWith("empty") || it.endsWith("nested") },
        )
    }

    @Test
    fun duplicateDetectionFindsIdenticalFiles() = runBlocking {
        val payload = ByteArray(8192) { (it % 97).toByte() }
        File(root, "a").mkdirs()
        File(root, "b").mkdirs()
        File(root, "a/dupe.bin").writeBytes(payload)
        File(root, "b/dupe.bin").writeBytes(payload)
        File(root, "a/unique.bin").writeBytes(ByteArray(8192) { 7 })
        val dups = repo.duplicates(root.absolutePath)
        assertEquals(1, dups.groups.size)
        assertEquals(2, dups.groups[0].paths.size)
        assertTrue(dups.totalWasted > 0L)
    }

    @Test
    fun archiveRoundTripOnDevice() = runBlocking {
        write("in/a.txt", 100)
        write("in/b.txt", 200)
        val zip = File(root, "out.zip").absolutePath
        val bytes = repo.createArchive(listOf(File(root, "in").absolutePath), zip, "zip")
        assertTrue("archive should have bytes", bytes > 0)

        val listed = repo.listArchive(zip)
        assertTrue(listed.entries.isNotEmpty())
        assertEquals("zip", listed.format.lowercase())

        val dest = File(root, "unpacked").absolutePath
        val extracted = repo.extractArchive(zip, dest)
        assertEquals(0, extracted.errors.size)
        assertEquals(100, File(dest, "in/a.txt").readBytes().size)
        assertEquals(200, File(dest, "in/b.txt").readBytes().size)
    }

    @Test
    fun safListOfGrantedTreeOrEmpty() {
        // Without a grant this must return an empty list rather than throwing;
        // the UI depends on that to distinguish "no grant" from "empty folder".
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val listed = cloud.fuss.app.SafBrowser.children(context, "content://com.android.externalstorage.documents/tree/primary%3ADownload")
        assertTrue(listed.size >= 0)
    }

    @Test
    fun sharedStorageIsReachableOrReportsDenial() = runBlocking {
        // On API 30+ some subdirectories are visible and others are not. Either
        // a listing or a clean permission error is acceptable; a crash is not.
        val shared = Environment.getExternalStorageDirectory().absolutePath
        try {
            val page = repo.list(shared, showHidden = false, sort = cloud.fuss.core.SortSpec.DEFAULT)
            assertTrue(page.entries.size >= 0)
        } catch (e: FussException) {
            assertTrue(e.code == "permission_denied" || e.code == "not_found")
        }
    }
}
