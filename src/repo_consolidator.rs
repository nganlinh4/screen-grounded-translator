use ignore::WalkBuilder;
use std::fs;
use std::collections::BTreeMap;

/// Consolidates repository files into a Vec of (path, content) pairs
pub fn consolidate_repo(path: &str) -> Vec<(String, String)> {
    let mut files = Vec::new();
    let mut skipped = 0;
    
    let walker = WalkBuilder::new(path)
        .standard_filters(true)
        .build();
    
    for result in walker {
        match result {
            Ok(entry) => {
                let path = entry.path();
                if path.is_file() {
                    match fs::read_to_string(path) {
                        Ok(content) => {
                            let rel_path = path
                                .strip_prefix(".")
                                .unwrap_or(path)
                                .to_string_lossy()
                                .replace("\\", "/");
                            
                            files.push((rel_path, content));
                        }
                        Err(_) => {
                            skipped += 1;
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
    
    if skipped > 0 {
        eprintln!("Skipped {} binary/unreadable files", skipped);
    }
    
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

/// Generates XML output in repomix v1.9.1 compatible format
pub fn generate_repomix_format(files: &[(String, String)]) -> String {
    let mut output = String::new();
    
    // Header summary
    output.push_str("This file is a merged representation of the entire codebase, combined into a single document by code-packer (compatible format).\n\n");
    
    output.push_str("<file_summary>\n");
    output.push_str("This section contains a summary of this file.\n\n");
    
    output.push_str("<purpose>\n");
    output.push_str("This file contains a packed representation of the entire repository's contents.\n");
    output.push_str("It is designed to be easily consumable by AI systems for analysis, code review,\n");
    output.push_str("or other automated processes.\n");
    output.push_str("</purpose>\n\n");
    
    output.push_str("<file_format>\n");
    output.push_str("The content is organized as follows:\n");
    output.push_str("1. This summary section\n");
    output.push_str("2. Repository information\n");
    output.push_str("3. Directory structure\n");
    output.push_str("4. Repository files (if enabled)\n");
    output.push_str("5. Multiple file entries, each consisting of:\n");
    output.push_str("   - File path as an attribute\n");
    output.push_str("   - Full contents of the file\n");
    output.push_str("</file_format>\n\n");
    
    output.push_str("<usage_guidelines>\n");
    output.push_str("- This file should be treated as read-only. Any changes should be made to the\n");
    output.push_str("  original repository files, not this packed version.\n");
    output.push_str("- When processing this file, use the file path to distinguish\n");
    output.push_str("  between different files in the repository.\n");
    output.push_str("- Be aware that this file may contain sensitive information. Handle it with\n");
    output.push_str("  the same level of security as you would the original repository.\n");
    output.push_str("</usage_guidelines>\n\n");
    
    output.push_str("<notes>\n");
    output.push_str("- Some files may have been excluded based on .gitignore rules\n");
    output.push_str("- Binary files are not included in this packed representation\n");
    output.push_str("- Files matching patterns in .gitignore are excluded\n");
    output.push_str("</notes>\n\n");
    
    output.push_str("</file_summary>\n\n");
    
    // Directory structure (strip src/ prefix like repomix does)
    output.push_str("<directory_structure>\n");
    let mut dir_tree = BTreeMap::new();
    for (path, _) in files {
        let clean_path = if path.starts_with("src/") {
            path.strip_prefix("src/").unwrap_or(&path).to_string()
        } else if path.starts_with("src\\") {
            path.strip_prefix("src\\").unwrap_or(&path).to_string()
        } else {
            path.clone()
        };
        dir_tree.insert(clean_path, true);
    }
    
    for path in dir_tree.keys() {
        output.push_str(path);
        output.push('\n');
    }
    output.push_str("</directory_structure>\n\n");
    
    // Files section with description (repomix format)
    output.push_str("<files>\n");
    output.push_str("This section contains the contents of the repository's files.\n\n");
    
    // File entries (repomix v1.9.1 format: plain text, no CDATA, no <content> wrapper, no <size>)
    for (path, content) in files {
        // Strip the input directory prefix (e.g., "src/" becomes just the relative path)
        let clean_path = if path.starts_with("src/") {
            path.strip_prefix("src/").unwrap_or(&path).to_string()
        } else if path.starts_with("src\\") {
            path.strip_prefix("src\\").unwrap_or(&path).to_string()
        } else {
            path.clone()
        };
        
        output.push_str(&format!("<file path=\"{}\">", clean_path));
        output.push('\n');
        output.push_str(content);
        output.push_str("</file>\n\n");
    }
    
    output.push_str("</files>\n");
    
    output
}
