use std::cell::RefCell;
use std::num::ParseIntError;
use std::rc::Rc;

use crate::stringtables::StringTable;

pub(crate) const INSTANCE_BASELINE_TABLE_NAME: &str = "instancebaseline";

#[derive(Default)]
pub(crate) struct InstanceBaseline {
    data: Vec<Option<Rc<RefCell<Vec<u8>>>>>,
}

impl InstanceBaseline {
    pub(crate) fn update(
        &mut self,
        string_table: &StringTable,
        classes: usize,
    ) -> Result<(), ParseIntError> {
        if self.data.len() < classes {
            self.data.resize(classes, None);
        }

        for (_entity_index, item) in string_table.items() {
            if let Some(string_bytes) = item.string.as_ref() {
                match std::str::from_utf8(string_bytes) {
                    Ok(string) => {
                        let class_id = string.parse::<i32>()?;
                        self.data[class_id as usize] = item.user_data.clone();
                    }
                    Err(_) => {
                        continue;
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn by_id(&self, class_id: i32) -> Option<std::cell::Ref<Vec<u8>>> {
        self.data
            .get(class_id as usize)
            .and_then(|v| v.as_ref())
            .map(|v| v.borrow())
    }

    /// clear clears underlying storage, but this has no effect on the allocated capacity.
    pub(crate) fn clear(&mut self) {
        self.data.clear();
    }
}
