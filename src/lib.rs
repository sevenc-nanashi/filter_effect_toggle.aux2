use aviutl2::{anyhow, tracing};

static GLOBAL_EDIT_HANDLE: aviutl2::generic::GlobalEditHandle =
    aviutl2::generic::GlobalEditHandle::new();

static EFFECTS: std::sync::LazyLock<Vec<aviutl2::generic::Effect>> =
    std::sync::LazyLock::new(|| GLOBAL_EDIT_HANDLE.get_effects());

#[aviutl2::plugin(GenericPlugin)]
struct FilterEffectToggleAux2;

impl aviutl2::generic::GenericPlugin for FilterEffectToggleAux2 {
    fn new(_info: aviutl2::AviUtl2Info) -> aviutl2::AnyResult<Self> {
        aviutl2::tracing_subscriber::fmt()
            .with_max_level(if cfg!(debug_assertions) {
                tracing::Level::DEBUG
            } else {
                tracing::Level::INFO
            })
            .event_format(aviutl2::logger::AviUtl2Formatter)
            .with_writer(aviutl2::logger::AviUtl2LogWriter)
            .init();
        Ok(Self)
    }

    fn plugin_info(&self) -> aviutl2::generic::GenericPluginTable {
        aviutl2::generic::GenericPluginTable {
            name: "filter_effect_toggle.aux2".to_string(),
            information: format!(
                "Toggle Between Filter Object and Filter Effect / v{} / https://github.com/sevenc-nanashi/filter_effect_toggle.aux2",
                env!("CARGO_PKG_VERSION")
            ),
        }
    }

    fn register(&mut self, registry: &mut aviutl2::generic::HostAppHandle) {
        GLOBAL_EDIT_HANDLE.init(registry.create_edit_handle());
        registry.register_menus::<Self>();
    }
}

#[aviutl2::generic::menus]
impl FilterEffectToggleAux2 {
    #[edit(name = "filter_effect_toggle.aux2\\フィルタオブジェクト ↔ フィルタ効果")]
    fn edit_toggle_filter_object_and_effect(&mut self) -> anyhow::Result<()> {
        self.object_toggle_filter_object_and_effect()
    }

    #[edit(name = "filter_effect_toggle.aux2\\フィルタオブジェクト → フィルタ効果")]
    fn edit_filter_object_to_effect(&mut self) -> anyhow::Result<()> {
        self.object_filter_object_to_effect()
    }

    #[edit(name = "filter_effect_toggle.aux2\\フィルタ効果 → フィルタオブジェクト")]
    fn edit_filter_effect_to_object(&mut self) -> anyhow::Result<()> {
        self.object_filter_effect_to_object()
    }

    #[object(name = "[filter_effect_toggle.aux2] フィルタオブジェクト ↔ フィルタ効果")]
    fn object_toggle_filter_object_and_effect(&mut self) -> anyhow::Result<()> {
        run_operations_to_selected_objects(|edit, object_handle| {
            let object = edit.object(object_handle);
            let alias = object.get_alias_parsed()?;
            let (object_type, _effect) = check_object_type(&alias)?.ok_or_else(|| {
                anyhow::anyhow!("The object is neither a Filter Object nor a Filter Effect")
            })?;
            match object_type {
                ObjectType::FilterObject => filter_object_to_effect(edit, object_handle),
                ObjectType::FilterEffect => filter_effect_to_object(edit, object_handle),
            }
        })
    }

    #[object(name = "[filter_effect_toggle.aux2] フィルタオブジェクト → フィルタ効果")]
    fn object_filter_object_to_effect(&mut self) -> anyhow::Result<()> {
        run_operations_to_selected_objects(filter_object_to_effect)
    }

    #[object(name = "[filter_effect_toggle.aux2] フィルタ効果 → フィルタオブジェクト")]
    fn object_filter_effect_to_object(&mut self) -> anyhow::Result<()> {
        run_operations_to_selected_objects(filter_effect_to_object)
    }
}

fn run_operations_to_selected_objects<F>(operation: F) -> anyhow::Result<()>
where
    F: Fn(
            &mut aviutl2::generic::EditSection,
            &aviutl2::generic::ObjectHandle,
        ) -> anyhow::Result<()>
        + Send
        + Sync
        + 'static,
{
    GLOBAL_EDIT_HANDLE.call_edit_section(|edit| {
        let mut errors = Vec::new();
        let mut objs = edit.get_selected_objects()?;
        if objs.is_empty() {
            if let Some(obj) = edit.get_focused_object()? {
                objs.push(obj);
            } else {
                anyhow::bail!("オブジェクトが選択されていません");
            }
        }
        tracing::info!("Applying operation to {} selected objects", objs.len());
        for obj in &objs {
            if let Err(e) = operation(edit, obj) {
                errors.push(anyhow::anyhow!(
                    "Failed to perform operation for object {:?}: {}",
                    obj,
                    e
                ));
            }
        }

        tracing::info!(
            "Operation completed with {} successes and {} failures",
            objs.len() - errors.len(),
            errors.len()
        );
        for error in &errors {
            tracing::error!("{}", error);
        }

        if errors.len() == objs.len() {
            anyhow::bail!("Failed to perform operation for all selected objects");
        }

        anyhow::Ok(())
    })?
}

fn filter_effect_to_object(
    edit: &mut aviutl2::generic::EditSection,
    object_handle: &aviutl2::generic::ObjectHandle,
) -> anyhow::Result<()> {
    let object = edit.object(object_handle);
    let alias = object.get_alias_parsed()?;
    let (object_type, effect) = check_object_type(&alias)?.ok_or_else(|| {
        anyhow::anyhow!("The object is neither a Filter Object nor a Filter Effect")
    })?;
    if object_type != ObjectType::FilterEffect {
        anyhow::bail!("The object is not a Filter Effect");
    }
    if !effect.flag.as_filter {
        anyhow::bail!("The effect cannot be used as a Filter Object");
    }

    let object_table = alias
        .get_table("Object")
        .ok_or_else(|| anyhow::anyhow!("[Object] table was not found"))?;
    let mut new_object_section = object_table.clone();
    for (i, table) in object_table.iter_subtables_as_array().enumerate() {
        new_object_section.insert_table(&(i + 1).to_string(), table.clone());
    }
    let mut filter_object_table = aviutl2::alias::Table::new();
    filter_object_table.insert_value("effect.name", "フィルタオブジェクト".to_string());
    new_object_section.insert_table("0", filter_object_table);

    let mut new_object_alias = alias.clone();
    new_object_alias.insert_table("Object", new_object_section);

    let positions = object.get_layer_frame()?;
    object.delete_object()?;
    edit.create_object_from_alias(
        &new_object_alias.to_string(),
        positions.layer,
        positions.start,
        positions.end - positions.start,
    )?;

    Ok(())
}
fn filter_object_to_effect(
    edit: &mut aviutl2::generic::EditSection,
    object_handle: &aviutl2::generic::ObjectHandle,
) -> anyhow::Result<()> {
    let object = edit.object(object_handle);
    let alias = object.get_alias_parsed()?;
    let (object_type, _effect) = check_object_type(&alias)?.ok_or_else(|| {
        anyhow::anyhow!("The object is neither a Filter Object nor a Filter Effect")
    })?;
    if object_type != ObjectType::FilterObject {
        anyhow::bail!("The object is not a Filter Object");
    }

    let object_table = alias
        .get_table("Object")
        .ok_or_else(|| anyhow::anyhow!("[Object] table was not found"))?;
    let mut new_object_section = object_table.clone();
    let mut last_table_index = 0;
    for (i, table) in object_table.iter_subtables_as_array().skip(1).enumerate() {
        new_object_section.insert_table(&i.to_string(), table.clone());
        last_table_index = i;
    }
    new_object_section.remove_table(&(last_table_index + 1).to_string());

    let mut new_object_alias = alias.clone();
    new_object_alias.insert_table("Object", new_object_section);

    let positions = object.get_layer_frame()?;
    object.delete_object()?;
    edit.create_object_from_alias(
        &new_object_alias.to_string(),
        positions.layer,
        positions.start,
        positions.end - positions.start,
    )?;

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObjectType {
    FilterObject,
    FilterEffect,
}

fn check_object_type(
    alias: &aviutl2::alias::Table,
) -> anyhow::Result<Option<(ObjectType, aviutl2::generic::Effect)>> {
    let first_object = alias
        .get_table("Object.0")
        .ok_or_else(|| anyhow::anyhow!("[Object.0] was not found"))?;
    let name = first_object
        .get_value("effect.name")
        .ok_or_else(|| anyhow::anyhow!("[Object.0] does not have effect.name"))?;

    if name == "フィルタオブジェクト" {
        let object_1 = alias
            .get_table("Object.1")
            .ok_or_else(|| anyhow::anyhow!("[Object.1] was not found"))?;
        let name_1 = object_1
            .get_value("effect.name")
            .ok_or_else(|| anyhow::anyhow!("[Object.1] does not have effect.name"))?;
        let effect = EFFECTS
            .iter()
            .find(|effect| {
                effect.effect_type == aviutl2::generic::EffectType::Filter && &effect.name == name_1
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Effect with name '{}' and type Filter was not found",
                    name_1
                )
            })?;
        Ok(Some((ObjectType::FilterObject, effect.clone())))
    } else if let Some(effect) = EFFECTS.iter().find(|effect| {
        effect.effect_type == aviutl2::generic::EffectType::Filter && &effect.name == name
    }) {
        Ok(Some((ObjectType::FilterEffect, effect.clone())))
    } else {
        Ok(None)
    }
}

aviutl2::register_generic_plugin!(FilterEffectToggleAux2);
