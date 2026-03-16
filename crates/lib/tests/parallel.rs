#![cfg(feature = "parallel")]

use std::io::Write;
use std::time::Instant;
use tempfile::NamedTempFile;

#[test]
fn from_paths_returns_correct_results_in_order() {
    let inputs = [
        ("a { color: red; }", "a {\n  color: red;\n}\n"),
        ("b { color: blue; }", "b {\n  color: blue;\n}\n"),
        ("c { d { color: green; } }", "c d {\n  color: green;\n}\n"),
    ];

    let files: Vec<NamedTempFile> = inputs
        .iter()
        .map(|(scss, _)| {
            let mut f = tempfile::Builder::new().suffix(".scss").tempfile().unwrap();
            f.write_all(scss.as_bytes()).unwrap();
            f
        })
        .collect();

    let paths: Vec<&str> = files.iter().map(|f| f.path().to_str().unwrap()).collect();

    let results = grass::from_paths(&paths, &grass::Options::default());

    assert_eq!(results.len(), inputs.len());
    for (result, (_, expected)) in results.iter().zip(inputs.iter()) {
        assert_eq!(result.as_ref().unwrap(), expected);
    }
}

#[test]
fn from_paths_error_does_not_affect_others() {
    let mut good_file = tempfile::Builder::new().suffix(".scss").tempfile().unwrap();
    good_file.write_all(b"a { color: red; }").unwrap();

    let paths = vec![
        good_file.path().to_str().unwrap().to_string(),
        "/nonexistent/path.scss".to_string(),
    ];

    let results = grass::from_paths(&paths, &grass::Options::default());

    assert_eq!(results.len(), 2);
    assert!(results[0].is_ok());
    assert!(results[1].is_err());
}

#[test]
fn from_paths_empty_input() {
    let results = grass::from_paths::<String>(&[], &grass::Options::default());
    assert!(results.is_empty());
}

/// Test that parallel compilation of USWDS produces identical output to sequential.
#[test]
fn parallel_uswds_matches_sequential() {
    let options = {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let root = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        grass::Options::default().load_path(root.join("prototype/packages"))
    };
    let path = &format!(
        "{}/prototype/packages/uswds/_index-direct.scss",
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .display()
    );

    let sequential = grass::from_path(path, &options).expect("sequential compile failed");
    let parallel =
        grass::from_path_parallel(path, &options, 8, 4).expect("parallel compile failed");

    if sequential != parallel {
        let seq_lines: Vec<&str> = sequential.lines().collect();
        let par_lines: Vec<&str> = parallel.lines().collect();
        eprintln!(
            "Size mismatch: seq={} par={} (diff={})",
            sequential.len(),
            parallel.len(),
            parallel.len() as i64 - sequential.len() as i64
        );
        eprintln!("Lines: seq={} par={}", seq_lines.len(), par_lines.len());
        for (i, (s, p)) in seq_lines.iter().zip(par_lines.iter()).enumerate() {
            if s != p {
                eprintln!(
                    "First diff at line {}: seq={:?}",
                    i + 1,
                    &s[..s.len().min(80)]
                );
                eprintln!("                       par={:?}", &p[..p.len().min(80)]);
                break;
            }
        }
        panic!("Parallel output differs from sequential");
    }
}

/// Benchmark parallel vs sequential compilation.
#[test]
fn parallel_uswds_timing() {
    let options = {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let root = std::path::Path::new(manifest_dir)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        grass::Options::default().load_path(root.join("prototype/packages"))
    };
    let path = &format!(
        "{}/prototype/packages/uswds/_index-direct.scss",
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .display()
    );

    // Warmup
    let _ = grass::from_path(path, &options);
    let _ = grass::from_path_parallel(path, &options, 0, 4);

    let t0 = Instant::now();
    let _seq = grass::from_path(path, &options).unwrap();
    let t_seq = t0.elapsed();

    let t0 = Instant::now();
    let _par = grass::from_path_parallel(path, &options, 0, 4).unwrap();
    let t_par = t0.elapsed();

    let speedup = t_seq.as_secs_f64() / t_par.as_secs_f64();
    eprintln!(
        "Sequential: {:.1}ms, Parallel: {:.1}ms, Speedup: {:.2}x",
        t_seq.as_secs_f64() * 1000.0,
        t_par.as_secs_f64() * 1000.0,
        speedup,
    );
}
