use std::collections::HashMap;
use std::sync::Arc;

use crate::render::{RenderError, RenderedIcon, render_icon};
use crate::state::IconState;

/// Memoized icon renders, keyed on the full [`IconState`] — style, rounded
/// percent, status, at-risk badge, mono and scale — mirroring `ClaudeMeter`'s
/// `IconCache`.
///
/// The key space is inherently bounded (101 percents x a handful of flags per
/// style), so entries are never evicted.
#[derive(Debug, Default)]
pub struct IconCache {
    entries: HashMap<IconState, Arc<RenderedIcon>>,
}

impl IconCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached render for `state`, rasterizing on first use.
    /// Failed renders are not cached, so a transient failure can retry.
    pub fn get_or_render(&mut self, state: IconState) -> Result<Arc<RenderedIcon>, RenderError> {
        if let Some(hit) = self.entries.get(&state) {
            return Ok(Arc::clone(hit));
        }
        let icon = Arc::new(render_icon(&state)?);
        self.entries.insert(state, Arc::clone(&icon));
        Ok(icon)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::state::{IconStyle, Scale};
    use meter_core::UsageStatus;
    use pretty_assertions::assert_eq;

    fn state(percent: u8) -> IconState {
        IconState {
            style: IconStyle::Battery,
            percent,
            status: UsageStatus::Safe,
            at_risk: false,
            mono: false,
            scale: Scale::X1,
        }
    }

    #[test]
    fn repeated_states_share_one_render() {
        let mut cache = IconCache::new();
        let first = cache.get_or_render(state(40)).unwrap();
        let second = cache.get_or_render(state(40)).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn distinct_states_render_separately() {
        let mut cache = IconCache::new();
        let a = cache.get_or_render(state(40)).unwrap();
        let b = cache.get_or_render(state(41)).unwrap();
        assert!(!Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 2);
    }
}
