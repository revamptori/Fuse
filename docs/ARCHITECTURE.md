# Architecture

Fuss is split into four layers with one rule: **filesystem logic lives in Rust,
Android specifics live in Kotlin, and the boundary between them is small.**

```
┌──────────────────────────────────────────────────────────────┐
│ Compose UI            ui/*.kt      screens, theme, formatting │
├──────────────────────────────────────────────────────────────┤
│ State & Android glue  app/*.kt     ViewModel, SAF, intents,   │
│                                    storage volumes, prefs     │
├──────────────────────────────────────────────────────────────┤
│ Wire layer            core/*.kt    FussNative (external funs),│
│                                    Decoder, FsRepository      │
├══════════════════════════════════════════════════════════════┤
│ JNI                   fuss-jni     argument marshal, byte     │
│                                    encoding, cancel handles   │
├──────────────────────────────────────────────────────────────┤
│ Engine                fuss-core    listing, fsops, archives,  │
│                                    search, analysis, mounts    │
└──────────────────────────────────────────────────────────────┘
```

## Why this split

* **Speed and control.** Directory scans, sorting, hashing, copying and archive
  parsing are CPU- and syscall-bound. Doing them in Rust with fixed buffers,
  `walkdir`, and no Java object churn keeps allocations flat regardless of file
  size and avoids GC pauses during large transfers.
* **Small JNI surface.** There are 30 native functions. Each returns either a
  primitive, a `String`, or one `ByteArray` of encoded results. No Java objects
  are constructed from Rust, no callbacks jump back into the JVM mid-operation,
  and no `jobject` arrays are held across calls. This keeps the
  attach/detach cost to one crossing per user action rather than one per file.
* **No Android in the engine.** `fuss-core` builds and is tested on a normal
  host (`cargo test`), which is why the engine has real unit tests and
  benchmarks. All Android knowledge — volumes, SAF documents, intent handling,
  permission state — stays in Kotlin where the framework APIs are.

## The boundary

### Calling direction

Kotlin → Rust only. `cloud.fuss.core.FussNative` is a Kotlin `object`, so its
`external fun`s are *instance* methods: JNI hands the native side the singleton
as the second argument (an object, not a class). The library therefore binds
every function explicitly in `JNI_OnLoad` via `RegisterNatives`, with the exact
descriptors the Kotlin declarations use, instead of relying on the symbolic
`Java_cloud_fuss_core_FussNative_<name>` lookup, which would assume a static
receiver and fail at the first call. A host test
(`registration_matches_kotlin_declarations`) parses `FussNative.kt` and fails the
build if the registration table and the Kotlin surface ever disagree. Every call is wrapped by `FsRepository`, which:

* switches to `Dispatchers.IO`,
* allocates a cancel handle for operations that can run long and frees it in a
  `finally` block (so the native handle table cannot leak),
* decodes the returned bytes into immutable Kotlin classes on the IO thread.

### Wire format

All integers are little-endian. Every reply is a status byte followed by either
a length-prefixed payload or an error code and message:

| Field | Encoding |
| --- | --- |
| status | `u8`: 0 = ok, 1 = error |
| payload | `u32` byte length, then the payload |
| error | code `string`, message `string` |
| string | `u32` UTF-8 length, then bytes |
| entry | name, `u8` flags (1 dir, 2 symlink, 4 stat-ok), `u8` kind, `u64` size, `i64` mtime, `u32` mode |

Entry records are packed like this so a 50 000-item directory decodes into
50 000 small objects without intermediate collections. `DecoderTest` pins the
layout from the Kotlin side; `codec.rs` has the matching Rust tests.

### Cancellation

Long operations take a `u64` handle. `newCancelToken` returns one,
`cancelToken(handle)` sets the flag, and the engine polls `CancelToken::is_cancelled`
at every file and every 64 KiB of data. Progress is read with `progress(handle)`.
Handles are `Arc<CancelToken>` entries in a mutex-protected table that is pruned
opportunistically; the Kotlin side frees every handle it creates.

## Threading

* All native calls happen on `Dispatchers.IO` inside `FsRepository`. The Compose
  layer never touches a blocking call.
* The ViewModel holds state in a `MutableStateFlow<BrowseUi>`; long work runs in
  `viewModelScope` and is cancelled when superseded (a new directory load
  cancels the previous one).
* Search debounces typing by 220 ms inside the composition effect, so a new
  keystroke cancels the previous scan via coroutine cancellation, which releases
  the native cancel handle in `withToken`'s `finally`.

## Storage and permissions

Android 11+ hides most of `/sdcard` from apps that lack `MANAGE_EXTERNAL_STORAGE`.
Fuss does not request that permission. Instead:

1. **Media directories** that the system exposes as real paths are browsed
   directly by the engine. `READ_MEDIA_*` / `READ_EXTERNAL_STORAGE` (≤ API 32)
   are declared only so previews and thumbnails can be produced.
2. **Everything else** is reached through SAF: `OpenDocumentTree` grants a tree
   URI, which `SafStore` persists with `takePersistableUriPermission`. Those
   trees are walked in Kotlin by `SafBrowser` (content:// has no path), can be
   previewed, and files can be streamed into app-visible storage so the engine
   can operate on them.

Revoked grants are handled where they are observed: `SafBrowser.children`
returns an empty list for an unreadable tree and the screen says so instead of
pretending the folder is empty, and `SafStore` prunes grants whose permissions no
longer exist.

## Error handling

The engine returns a stable `ErrorCode` (`not_found`, `permission_denied`,
`already_exists`, `no_space`, `cross_device`, `cancelled`, `corrupt_archive`, …)
plus a message and path. Kotlin turns the code into a short user-facing sentence
(`FussException.userMessage`). Batch operations (copy/move/delete) never throw
away partial results: they return counts plus the first error, so the UI can say
"copied 12 files, 1 failed: permission denied" rather than claiming success.
Archive extraction reports per-entry errors the same way.

## Dependencies, and why each one exists

### Rust (`fuss-core`, `fuss-jni`)

| Crate | Reason |
| --- | --- |
| `jni` | the only supported JNI binding; used solely by `fuss-jni` |
| `miniz_oxide` | pure-Rust DEFLATE — ZIP compression/extraction and GZIP, no C toolchain needed in the NDK build |
| `walkdir` | iterative, symlink-aware directory walking for analysis and search |
| `thiserror` | derive for the engine's error type; no runtime cost |
| `log` | a facade only; `fuss-jni` does not install a logger, so it is inert |

Everything else in `rust/Cargo.lock` is a transitive dependency of those five.

### Kotlin

| Artifact | Reason |
| --- | --- |
| Compose BOM + `ui`, `foundation`, `material3` | the required UI toolkit and design system |
| `material-icons-extended` | outlined pictograms; roughly 1 MB of vector code, and it is the only icon source |
| `activity-compose` | `setContent`, `BackHandler`, `rememberLauncherForActivityResult` |
| `lifecycle-viewmodel-compose` | `viewModel()` for the shared `BrowseViewModel` |
| `core-ktx` | `FileProvider`, edge-to-edge |
| `documentfile` | framework wrapper for SAF tree enumeration |
| `coil-compose` | image thumbnails with memory/disk caching and correct downsampling; hand-rolling this would be more code and more bugs |
| `junit` (test) | the only test dependency |

No networking, database, DI, analytics, or navigation library is present.

## Performance decisions

* Listing returns one page plus `has_more`; the UI can render the first screen of
  a huge directory before the scan finishes.
* Sorting happens in Rust before the page is encoded.
* Copying and archive I/O use fixed 64 KiB (or larger) buffers and never buffer a
  whole file.
* Release builds use `opt-level = "s"`, fat LTO, one codegen unit and full symbol
  stripping; `libfuss_core.so` is also run through `llvm-strip`.
* The Activity does no work in `onCreate` beyond installing Compose; the
  Application class only registers a crash handler.

## Testing strategy

* **Rust unit tests** cover the engine directly, including filesystem behaviour
  in temporary directories (collisions, recursion, cancellation, archive
  round-trips, corrupt input).
* **JNI host tests** exercise the cancel-handle table and the codec from the Rust
  side.
* **Kotlin unit tests** pin the wire decoder and the formatters.
* CI builds all three ABIs, assembles debug and release APKs, and runs both test
  suites. Device-level integration tests are described in
  [docs/TESTING.md](docs/TESTING.md) with the commands to run them on a real
  phone or emulator.
