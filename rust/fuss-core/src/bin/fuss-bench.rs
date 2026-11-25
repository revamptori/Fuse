//! Host-side benchmark for the engine's critical paths.
//!
//! Usage: `fuss-bench [workdir]`
//!
//! Produces a table of operations with median timings over several runs, using
//! synthetic trees. The aim is to catch regressions and to document the
//! engine's performance honestly: numbers come from this machine, not from a
//! phone, and the docs say so.

use fuss_core::archive;
use fuss_core::cancel::CancelToken;
use fuss_core::entry::{sort_entries, SortKey};
use fuss_core::fsops::{copy_file, CollisionMode, OpStats};
use fuss_core::listing::{list_dir, ListOptions};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let root = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("fuss-bench"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("create bench dir");
    println!("fuss-bench: engine {}", fuss_core::VERSION);
    println!("workdir: {}", root.display());
    println!();
    println!("{:<44} {:>12} {:>12} {:>10}", "operation", "median", "p90", "iterations");
    println!("{}", "-".repeat(82));

    let cancel = CancelToken::new();

    // Directory listing: 10 000 files in one directory.
    let big = root.join("big");
    fs::create_dir_all(&big).unwrap();
    for i in 0..10_000 {
        fs::write(big.join(format!("file_{i:05}.txt")), b"x").unwrap();
    }
    bench("list 10k files (no stat sort)", 20, || {
        let l = list_dir(&big, ListOptions::default(), &cancel).unwrap();
        std::hint::black_box(l.entries.len());
    });
    bench("list 10k files + natural sort", 20, || {
        let mut l = list_dir(&big, ListOptions::default(), &cancel).unwrap();
        sort_entries(&mut l.entries, SortKey::Name, true);
        std::hint::black_box(l.entries[0].name.len());
    });
    bench("list 10k files + show hidden", 20, || {
        let l = list_dir(&big, ListOptions { show_hidden: true, ..Default::default() }, &cancel).unwrap();
        std::hint::black_box(l.entries.len());
    });

    // Stat cost of a small directory in a metadata-heavy loop.
    bench("sort 10k entries by size", 20, || {
        let mut l = list_dir(&big, ListOptions::default(), &cancel).unwrap();
        sort_entries(&mut l.entries, SortKey::Size, true);
        std::hint::black_box(l.entries[0].size);
    });

    // Small files: copy 500 x 4 KiB.
    let small_src = root.join("small_src");
    fs::create_dir_all(&small_src).unwrap();
    for i in 0..500 {
        let mut f = fs::File::create(small_src.join(format!("s{i}.bin"))).unwrap();
        f.write_all(&vec![7u8; 4096]).unwrap();
    }
    let small_dst = root.join("small_dst");
    fs::create_dir_all(&small_dst).unwrap();
    bench("copy 500 x 4 KiB files", 5, || {
        for i in 0..500 {
            copy_file(&small_src.join(format!("s{i}.bin")), &small_dst.join(format!("s{i}.bin")), CollisionMode::Overwrite, &cancel).unwrap();
        }
    });

    // Large file: 256 MiB streamed copy.
    let large = root.join("large.bin");
    {
        let mut f = fs::File::create(&large).unwrap();
        let block = vec![0x5Au8; 1024 * 1024];
        for _ in 0..256 {
            f.write_all(&block).unwrap();
        }
        f.sync_all().unwrap();
    }
    let large_dst = root.join("large_copy.bin");
    bench("copy 256 MiB (streamed)", 3, || {
        copy_file(&large, &large_dst, CollisionMode::Overwrite, &cancel).unwrap();
    });

    // Directory size walk over the 10k-file directory.
    bench("dir_size 10k files", 10, || {
        let s = fuss_core::analyze::dir_size(&big, &cancel).unwrap();
        std::hint::black_box(s.bytes);
    });

    // Search over the 10k-file directory.
    bench("search 'file_099' in 10k", 10, || {
        let r = fuss_core::search::search(&big, &fuss_core::search::SearchOptions::new("file_099"), &cancel).unwrap();
        std::hint::black_box(r.entries.len());
    });

    // ZIP create + list of the 500 small files.
    let zip_path = root.join("bench.zip");
    bench("zip create 500 x 4 KiB", 3, || {
        archive::create_zip(&[small_src.clone()], &zip_path, &cancel).unwrap();
    });
    bench("zip list 500 entries", 10, || {
        let l = archive::list(&zip_path, &cancel).unwrap();
        std::hint::black_box(l.entries.len());
    });

    // Recursive delete of the destination tree.
    bench("delete tree (500 files)", 3, || {
        copy_file(&small_src.join("s0.bin"), &small_dst.join("seed.bin"), CollisionMode::Overwrite, &cancel).unwrap();
        let mut stats = OpStats::default();
        fuss_core::fsops::delete_path(&small_dst, &cancel, &mut stats).unwrap();
        fs::create_dir_all(&small_dst).unwrap();
    });

    let _ = fs::remove_dir_all(&root);
    println!("{}", "-".repeat(82));
    if let Ok(rss) = fs::read_to_string("/proc/self/status") {
        for line in rss.lines() {
            if line.starts_with("VmHWM") || line.starts_with("VmRSS") {
                println!("{}", line.trim());
            }
        }
    }
    let _ = Path::new("/");
}

fn bench<F: FnMut()>(name: &str, iters: usize, mut f: F) {
    // Warm-up run to page in code and filesystem caches consistently.
    f();
    let mut times = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t = Instant::now();
        f();
        times.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median = times[times.len() / 2];
    let p90 = times[(times.len() as f64 * 0.9) as usize % times.len()];
    println!("{:<44} {:>10.2}ms {:>10.2}ms {:>10}", name, median, p90, iters);
}
