pub mod body;
pub mod conversion;
pub mod node;
pub mod utils;
pub mod viewer;

pub use conversion::{blocks_to_snarl, snarl_to_graph};
pub use node::ChainNode;
pub use utils::request_node_graph_view_reset;
pub use viewer::ChainViewer;

use crate::gui::locale::LocaleText;
use eframe::egui;
use egui_snarl::ui::SnarlStyle;
use egui_snarl::{InPinId, OutPinId, Snarl};
use std::collections::HashMap;

/// Render the node graph in the preset editor
pub fn render_node_graph(
    ui: &mut egui::Ui,
    snarl: &mut Snarl<ChainNode>,
    ui_language: &str,
    prompt_mode: &str,
    use_groq: bool,
    use_gemini: bool,
    use_openrouter: bool,
    use_ollama: bool,
    preset_type: &str,
    text: &LocaleText,
) -> bool {
    let mut viewer = ChainViewer::new(
        text,
        ui_language,
        prompt_mode,
        use_groq,
        use_gemini,
        use_openrouter,
        use_ollama,
        preset_type,
    );
    let style = SnarlStyle::default();

    snarl.show(&mut viewer, &style, egui::Id::new("chain_graph"), ui);

    // Constraint Enforcement: Post-update cleanup
    // 1. No self-loops
    // 2. Single connection per input
    // 3. (Added) Single connection per output? No, fan-out is allowed.

    let mut to_disconnect = Vec::new();
    let mut input_count: HashMap<InPinId, Vec<OutPinId>> = HashMap::new();

    for (out, inp) in snarl.wires() {
        if out.node == inp.node {
            to_disconnect.push((out, inp));
        } else {
            input_count.entry(inp).or_default().push(out);
        }
    }

    for (_inp, sources) in input_count {
        if sources.len() > 1 {
            // More than 1 connection: Keep the last one encountered (arbitrary but consistent)
            // discard all but last
            for &src in sources.iter().take(sources.len() - 1) {
                // We re-construct iterator to find inp... wait sources is OutPinIDs
                // We need (OutPinId, InPinId) to disconnect
                // But disconnect takes (Out, In)? Yes.
                to_disconnect.push((src, _inp));
            }
        }
    }

    let mut cleanup_changed = false;
    for (out, inp) in to_disconnect {
        snarl.disconnect(out, inp);
        cleanup_changed = true;
    }

    viewer.changed || cleanup_changed
}
