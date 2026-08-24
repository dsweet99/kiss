use super::{BinaryIdObjectMap, resolve_objects_for_profdata};
use crate::rust_llvm_cov_runner::execute_or_reuse::batch_export_tools::ExportTools;
use crate::rust_llvm_cov_runner::test_support::write_executable;

#[test]
#[cfg(unix)]
fn resolve_objects_for_profdata_tolerates_orphan_ids_when_seeds_cover() {
    let tmp = tempfile::tempdir().unwrap();
    let llvm_profdata = write_executable(
        tmp.path().join("llvm-profdata"),
        "#!/bin/sh\nif [ \"$1\" = show ]; then printf 'Binary IDs:\\ncafebabe\\ndeadbeef\\n'; exit 0; fi\nexit 1\n",
    );
    let tools = ExportTools {
        llvm_profdata,
        llvm_cov: write_executable(tmp.path().join("llvm-cov"), "#!/bin/sh\nexit 1\n"),
        llvm_readobj: write_executable(
            tmp.path().join("llvm-readobj"),
            "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
        ),
    };
    let profdata = tmp.path().join("instance.profdata");
    let seed = tmp.path().join("seed-object");
    std::fs::write(&profdata, b"profile").unwrap();
    std::fs::write(&seed, b"object").unwrap();
    let catalog = vec![seed.clone()];
    let map = BinaryIdObjectMap::build(&tools, &catalog).expect("map");
    let resolved = resolve_objects_for_profdata(
        &tools,
        &profdata,
        &catalog,
        std::slice::from_ref(&seed),
        Some(&map),
    )
    .expect("orphan deadbeef should be tolerated when seed cafebabe covers");
    assert_eq!(resolved, vec![seed]);
}

#[test]
#[cfg(unix)]
fn metamorphic_orphan_ids_do_not_change_seed_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let llvm_profdata = write_executable(
        tmp.path().join("llvm-profdata"),
        "#!/bin/sh\nif [ \"$1\" = show ]; then cat \"$0.ids\"; exit 0; fi\nexit 1\n",
    );
    let ids_path = format!("{}.ids", llvm_profdata.display());
    std::fs::write(&ids_path, b"Binary IDs:\ncafebabe\n").unwrap();
    let tools = ExportTools {
        llvm_profdata: llvm_profdata.clone(),
        llvm_cov: write_executable(tmp.path().join("llvm-cov"), "#!/bin/sh\nexit 0\n"),
        llvm_readobj: write_executable(
            tmp.path().join("llvm-readobj"),
            "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
        ),
    };
    let profdata = tmp.path().join("instance.profdata");
    let seed = tmp.path().join("seed-object");
    std::fs::write(&profdata, b"profile").unwrap();
    std::fs::write(&seed, b"object").unwrap();
    let catalog = vec![seed.clone()];
    let map = BinaryIdObjectMap::build(&tools, &catalog).expect("map");
    let baseline = resolve_objects_for_profdata(
        &tools,
        &profdata,
        &catalog,
        std::slice::from_ref(&seed),
        Some(&map),
    )
    .expect("baseline");
    std::fs::write(&ids_path, b"Binary IDs:\ncafebabe\ndeadbeef\norphan1\n").unwrap();
    let with_orphans = resolve_objects_for_profdata(
        &tools,
        &profdata,
        &catalog,
        std::slice::from_ref(&seed),
        Some(&map),
    )
    .expect("with orphans");
    assert_eq!(baseline, with_orphans);
}

#[test]
#[cfg(unix)]
fn map_miss_does_not_rescan_catalog_with_llvm_readobj() {
    let tmp = tempfile::tempdir().unwrap();
    let counter = tmp.path().join("readobj.count");
    std::fs::write(&counter, b"0").unwrap();
    let tools = counting_readobj_tools(tmp.path(), &counter);
    let (profdata, seed, catalog) = seed_catalog_with_decoys(tmp.path(), 40);
    let map = BinaryIdObjectMap::build(&tools, &catalog).expect("map");
    let after_map: u64 = std::fs::read_to_string(&counter)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let resolved = resolve_objects_for_profdata(
        &tools,
        &profdata,
        &catalog,
        std::slice::from_ref(&seed),
        Some(&map),
    )
    .expect("orphans tolerated via seed");
    assert_eq!(resolved, vec![seed]);
    let after_resolve: u64 = std::fs::read_to_string(&counter)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(
        after_resolve <= after_map + 2,
        "expected ≤2 extra llvm-readobj calls after map build, got map={after_map} resolve={after_resolve}"
    );
}

#[cfg(unix)]
fn counting_readobj_tools(tmp: &std::path::Path, counter: &std::path::Path) -> ExportTools {
    let llvm_readobj = write_executable(
        tmp.join("llvm-readobj"),
        &format!(
            "#!/bin/sh
c=$(cat '{c}'); echo $((c+1)) > '{c}';
idfile=\"$2.id\"
\
if [ -f \"$idfile\" ]; then cat \"$idfile\"; else printf 'Build ID: missing\n'; fi
exit 0
",
            c = counter.display()
        ),
    );
    ExportTools {
        llvm_profdata: write_executable(
            tmp.join("llvm-profdata"),
            "#!/bin/sh
if [ \"$1\" = show ]; then printf 'Binary IDs:\ncafebabe\naaaaaaaa\nbbbbbbbb\n'; exit 0; fi
exit 1
",
        ),
        llvm_cov: write_executable(
            tmp.join("llvm-cov"),
            "#!/bin/sh
exit 1
",
        ),
        llvm_readobj,
    }
}

#[cfg(unix)]
fn seed_catalog_with_decoys(
    tmp: &std::path::Path,
    decoys: usize,
) -> (
    std::path::PathBuf,
    std::path::PathBuf,
    Vec<std::path::PathBuf>,
) {
    let profdata = tmp.join("instance.profdata");
    let seed = tmp.join("seed-object");
    std::fs::write(&profdata, b"profile").unwrap();
    std::fs::write(&seed, b"object").unwrap();
    std::fs::write(
        format!("{}.id", seed.display()),
        b"Build ID: cafebabe
",
    )
    .unwrap();
    let mut catalog = vec![seed.clone()];
    for i in 0..decoys {
        let decoy = tmp.join(format!("decoy-{i}"));
        std::fs::write(&decoy, b"object").unwrap();
        std::fs::write(
            format!("{}.id", decoy.display()),
            format!(
                "Build ID: decoy{i:04x}
"
            ),
        )
        .unwrap();
        catalog.push(decoy);
    }
    (profdata, seed, catalog)
}

#[test]
#[cfg(unix)]
fn fuzz_orphan_id_subsets_preserve_seed_when_covered() {
    let seed_value: u64 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    println!("fuzz_orphan_id_subsets seed={seed_value}");
    let mut rng = seed_value;
    for _ in 0..32 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
        let orphan_count = (rng % 5) as usize;
        let tmp = tempfile::tempdir().unwrap();
        let mut ids = String::from("Binary IDs:\ncafebabe\n");
        for i in 0..orphan_count {
            ids.push_str(&format!("orphan{i:08x}\n"));
        }
        let llvm_profdata = write_executable(
            tmp.path().join("llvm-profdata"),
            "#!/bin/sh\nif [ \"$1\" = show ]; then cat \"$0.ids\"; exit 0; fi\nexit 1\n",
        );
        std::fs::write(format!("{}.ids", llvm_profdata.display()), ids).unwrap();
        let tools = ExportTools {
            llvm_profdata,
            llvm_cov: write_executable(tmp.path().join("llvm-cov"), "#!/bin/sh\nexit 0\n"),
            llvm_readobj: write_executable(
                tmp.path().join("llvm-readobj"),
                "#!/bin/sh\nprintf 'Build ID: cafebabe\\n'\nexit 0\n",
            ),
        };
        let profdata = tmp.path().join("instance.profdata");
        let seed = tmp.path().join("seed-object");
        std::fs::write(&profdata, b"profile").unwrap();
        std::fs::write(&seed, b"object").unwrap();
        let catalog = vec![seed.clone()];
        let map = BinaryIdObjectMap::build(&tools, &catalog).unwrap();
        let resolved = resolve_objects_for_profdata(
            &tools,
            &profdata,
            &catalog,
            std::slice::from_ref(&seed),
            Some(&map),
        )
        .expect("covered seed must resolve for any orphan subset");
        assert_eq!(resolved, vec![seed]);
    }
}
