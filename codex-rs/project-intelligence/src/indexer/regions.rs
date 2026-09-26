use crate::ContextMapCoverage;

const LINES_PER_REGION: usize = 64;
const MAX_REGIONS_PER_FILE: usize = 64;
pub(super) const MAX_REGION_DESCRIPTION_BYTES: usize = 4_096;
pub(super) const MAX_PROJECT_REGIONS: usize = 50_000;

pub(super) struct ScannedRegion {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) description: String,
    pub(super) coverage: ContextMapCoverage,
}

pub(super) fn scan_regions(
    relative_path: &str,
    text: Option<&str>,
    exact_text: bool,
) -> (Vec<ScannedRegion>, bool) {
    let Some(text) = text else {
        return (Vec::new(), false);
    };
    let lines = text.lines().collect::<Vec<_>>();
    let mut regions = Vec::new();
    let mut heading = None;
    let mut line_index = 0;
    while line_index < lines.len() && regions.len() < MAX_REGIONS_PER_FILE {
        let start_line = line_index + 1;
        let inherited_heading = heading;
        let maximum_end = (line_index + LINES_PER_REGION).min(lines.len());
        let mut end_index = line_index;
        let mut description = String::new();
        while end_index < maximum_end {
            let candidate = describe_region(
                relative_path,
                start_line,
                end_index + 1,
                inherited_heading,
                &lines[line_index..=end_index],
            );
            if candidate.len() > MAX_REGION_DESCRIPTION_BYTES && end_index > line_index {
                break;
            }
            description = candidate;
            end_index += 1;
            if description.len() > MAX_REGION_DESCRIPTION_BYTES {
                break;
            }
        }
        let complete = exact_text && description.len() <= MAX_REGION_DESCRIPTION_BYTES;
        description.truncate(description.floor_char_boundary(MAX_REGION_DESCRIPTION_BYTES));
        heading = lines[line_index..end_index]
            .iter()
            .rev()
            .find_map(|line| line.trim().starts_with('#').then(|| line.trim()))
            .or(heading);
        regions.push(ScannedRegion {
            start_line,
            end_line: end_index,
            description,
            coverage: if complete {
                ContextMapCoverage::Complete
            } else {
                ContextMapCoverage::Partial
            },
        });
        line_index = end_index;
    }
    (regions, line_index < lines.len())
}

fn describe_region(
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    inherited_heading: Option<&str>,
    lines: &[&str],
) -> String {
    let mut description = format!("{relative_path}:{start_line}-{end_line}");
    if let Some(heading) = inherited_heading {
        description.push_str(" | ");
        description.push_str(heading);
    }
    description.push_str(" | ");
    description.push_str(&lines.join("\n"));
    description
}
