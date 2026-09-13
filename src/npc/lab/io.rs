use std::{fs, path::Path};

use super::model::{LabArtifact, LabError, LabReport};

pub fn write_artifact(path: impl AsRef<Path>, artifact: &LabArtifact) -> Result<(), LabError> {
    let json = serde_json::to_string_pretty(artifact)
        .map_err(|error| LabError::Serialization(error.to_string()))?;
    fs::write(path, json).map_err(|error| LabError::Io(error.to_string()))
}

pub fn read_artifact(path: impl AsRef<Path>) -> Result<LabArtifact, LabError> {
    let json = fs::read_to_string(path).map_err(|error| LabError::Io(error.to_string()))?;
    serde_json::from_str(&json).map_err(|error| LabError::Serialization(error.to_string()))
}

pub fn write_svg(path: impl AsRef<Path>, report: &LabReport) -> Result<(), LabError> {
    let mut output = String::from(
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="-45 -45 90 90">
<rect x="-45" y="-45" width="90" height="90" fill="#10141c"/>
<g fill="none" stroke-width="0.22" stroke-linecap="round" stroke-linejoin="round">
"##,
    );
    for (id, points) in &report.trace.trajectories {
        if points.len() < 2 {
            continue;
        }
        output.push_str(&format!(
            r#"<polyline stroke="hsl({},75%,60%)" points=""#,
            (*id as usize * 67) % 360
        ));
        for point in points {
            output.push_str(&format!("{:.3},{:.3} ", point[0], -point[1]));
        }
        output.push_str("\"/>\n");
    }
    output.push_str("</g>\n</svg>\n");
    fs::write(path, output).map_err(|error| LabError::Io(error.to_string()))
}
