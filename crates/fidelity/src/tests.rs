use std::collections::HashSet;
use std::path::PathBuf;

use serde_json::{Value, json};

use crate::{Outcome, cases, compare, export_python, run_all, run_case};

fn docs_fidelity() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/fidelity")
}

#[test]
fn ids_are_unique_and_every_case_measures_something() {
    let all = cases();
    let ids: HashSet<&str> = all.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids.len(), all.len(), "duplicate case id");
    assert!(all.len() > 60, "{} cases", all.len());
    for c in &all {
        assert!(!c.samples.is_empty() || !c.knob_samples.is_empty(), "{} measures nothing", c.id);
        assert!(c.nodes.len() <= 8, "{}: keep cases small (Nuke Non-commercial retrieves at most 10 nodes)", c.id);
    }
}

#[test]
fn every_implemented_case_runs_in_nukecraft() {
    for c in cases().iter().filter(|c| c.nukecraft) {
        let m = run_case(c);
        assert!(m.error.is_none(), "{}: {:?}", c.id, m.error);
        assert!(!m.values.is_empty(), "{}", c.id);
        assert!(m.values.values().all(|v| v.is_finite()), "{}: non-finite values", c.id);
    }
}

#[test]
fn spot_checks_of_what_nukecraft_measures() {
    let all = cases();
    let get = |id: &str, key: &str| -> f64 {
        let c = all.iter().find(|c| c.id == id).unwrap();
        *run_case(c).values.get(key).unwrap_or_else(|| panic!("{id}: no {key}"))
    };
    // The impulse moved by a whole pixel would land on (31, 31) exactly with Impulse filtering.
    assert_eq!(get("transform.filter.impulse", "px:Dot:1:31:31:rgba.red"), 1.0);
    // A over B (alpha 0.5): 0.6 + 0.2 * 0.5.
    assert!((get("merge.over", "px:M:1:5:5:rgba.red") - 0.7).abs() < 1e-6);
    // Ramp linear at the pixel centre y = 30.5: 30.5 / 64.
    assert!((get("ramp.linear", "px:R:1:10:30:rgba.red") - 30.5 / 64.0).abs() < 1e-6);
    // The knob curve passes through its keys.
    assert_eq!(get("animation.default", "knob:T:translate:10:0"), 100.0);
}

#[test]
fn compare_matches_itself_and_catches_differences() {
    let all: Vec<_> = cases().into_iter().filter(|c| c.id.starts_with("merge.over") || c.id == "blur.gaussian.60" || c.id == "merge.soft-light").collect();
    let mut oracle = run_all(&all);
    oracle["cases"]["merge.soft-light"] = json!({ "values": { "px:M:1:5:5:rgba.red": 0.5 }, "error": null });
    let report = compare(&all, &oracle);
    for c in &report.cases {
        match c.id.as_str() {
            "merge.soft-light" => assert_eq!(c.outcome, Outcome::NotImplemented),
            _ => assert!(matches!(c.outcome, Outcome::Match { .. }), "{}: {:?}", c.id, c.outcome),
        }
    }
    // Nudge one Nuke value: that case now differs, by that much.
    oracle["cases"]["merge.over"]["values"]["px:M:1:5:5:rgba.red"] = json!(0.75);
    let report = compare(&all, &oracle);
    let over = report.cases.iter().find(|c| c.id == "merge.over").unwrap();
    match &over.outcome {
        Outcome::Differs { max_error, mismatched, worst, .. } => {
            assert!((max_error - 0.05).abs() < 1e-6);
            assert_eq!(*mismatched, 1);
            assert_eq!(worst, "px:M:1:5:5:rgba.red");
        }
        other => panic!("{other:?}"),
    }
    assert!(report.markdown().contains("**differs**"));
    // A case Nuke couldn't run, and one with no numbers at all.
    oracle["cases"]["merge.over"] = json!({ "values": {}, "error": "Traceback…\nKeyError: no knob" });
    oracle["cases"].as_object_mut().unwrap().remove("blur.gaussian.60");
    let report = compare(&all, &oracle);
    assert!(matches!(&report.cases.iter().find(|c| c.id == "merge.over").unwrap().outcome, Outcome::NukeError(e) if e.contains("KeyError")));
    assert_eq!(report.cases.iter().find(|c| c.id == "blur.gaussian.60").unwrap().outcome, Outcome::NoOracle);
}

#[test]
fn defaults_are_compared() {
    let oracle = json!({
        "cases": {},
        "defaults": {
            "Blur": { "size": [0.0, 0.0], "filter": "gaussian", "crop": true, "mix": 1.0, "channels": "rgba" },
            "Grade": { "gamma": 1.0, "white": [1.0, 1.0, 1.0, 1.0], "black_clamp": 0.0 },
        }
    });
    let report = compare(&[], &oracle);
    // Grade's black_clamp: NukeCraft true, "Nuke" (here) off.
    assert!(report.defaults.iter().any(|d| d.class == "Grade" && d.knob == "black_clamp"));
    assert!(!report.defaults.iter().any(|d| d.class == "Blur"), "{:?}", report.defaults);
    // Knobs this oracle doesn't list for Grade are reported as possibly misnamed.
    assert!(report.missing_knobs.iter().any(|(c, k)| c == "Grade" && k == "blackpoint"));
}

/// Runs the generated Nuke script against a mock `nuke` module (when python3 is installed): it
/// must build every case, measure every value and write the oracle without errors.
#[test]
fn the_nuke_script_runs_against_a_mock_nuke() {
    if std::process::Command::new("python3").arg("--version").output().is_err() {
        eprintln!("python3 not found: skipping");
        return;
    }
    let all = cases();
    let script = export_python(&all).unwrap();
    let dir = std::env::temp_dir().join(format!("nukecraft-fidelity-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("nukecraft_fidelity.py");
    std::fs::write(&path, &script).unwrap();
    let out = dir.join("oracle.json");
    let mock = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/mock");
    let program = format!(
        "import sys; sys.path.insert(0, {mock:?}); exec(open({path:?}).read()); import nuke; \
         assert nuke.stats['created'] == nuke.stats['deleted'], nuke.stats; print('nodes', nuke.stats)",
        mock = mock.display().to_string(),
        path = path.display().to_string()
    );
    let run = std::process::Command::new("python3").arg("-S").arg("-c").arg(&program).env("NUKECRAFT_ORACLE_OUT", &out).output().unwrap();
    assert!(run.status.success(), "{}\n{}", String::from_utf8_lossy(&run.stdout), String::from_utf8_lossy(&run.stderr));
    let oracle: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    for c in &all {
        let entry = &oracle["cases"][&c.id];
        assert!(entry["error"].is_null(), "{}: {}", c.id, entry["error"]);
        // Every value NukeCraft measures, the script measures too, under the same key.
        if c.nukecraft {
            for key in run_case(c).values.keys() {
                assert!(entry["values"].get(key).is_some(), "{}: the Nuke script doesn't measure {key}", c.id);
            }
        }
    }
    assert!(oracle["defaults"]["Transform"]["filter"].is_number() || oracle["defaults"]["Transform"]["filter"].is_string());
}

/// Once real oracle numbers are committed, the cases that matched keep matching.
#[test]
fn committed_oracle_regressions() {
    let oracle_path = docs_fidelity().join("oracle.json");
    let Ok(text) = std::fs::read_to_string(&oracle_path) else {
        eprintln!("no docs/fidelity/oracle.json yet: nothing to hold to");
        return;
    };
    let oracle: Value = serde_json::from_str(&text).unwrap();
    let held: Vec<String> = std::fs::read_to_string(docs_fidelity().join("matching.txt"))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let report = compare(&cases(), &oracle);
    for id in held {
        let c = report.cases.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("matching.txt names unknown case {id}"));
        assert!(matches!(c.outcome, Outcome::Match { .. }), "{id} matched Nuke before and no longer does: {:?}", c.outcome);
    }
}
