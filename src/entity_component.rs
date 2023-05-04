use std::{
    any::Any,
    cell::UnsafeCell,
    collections::{BTreeMap, BTreeSet},
};

use uuid::Uuid;

pub struct Entity {
    components: BTreeMap<Uuid, ComponentBase>,
    uuid: Uuid,
}
impl Entity {
    pub fn new() -> Self {
        Self {
            components: BTreeMap::new(),
            uuid: Uuid::new_v4(),
        }
    }

    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    pub fn add_component<T: Component>(&mut self, component: T) {
        debug_assert!(
            self.components
                .insert(T::UUID, component.create_component_base())
                .is_none(),
            "can only add component once"
        );
    }

    pub fn delete_component(&mut self, uuid: &Uuid) -> Option<ComponentBase> {
        self.components.remove(uuid)
    }

    pub fn has_component<T: Component + 'static>(&self) -> bool {
        self.components.contains_key(&T::UUID)
    }

    pub fn get_component<T: Component + 'static>(&self) -> Option<&T> {
        if let Some(component) = self.components.get(&T::UUID) {
            #[cfg(debug_assertions)]
            return component.inner.downcast_ref::<T>();
            #[cfg(not(debug_assertions))]
            return Some(unsafe { component.inner.downcast_ref_unchecked::<T>() });
        } else {
            None
        }
    }

    pub fn get_component_mut<T: Component + 'static>(&mut self) -> Option<&mut T> {
        if let Some(component) = self.components.get_mut(&T::UUID) {
            #[cfg(debug_assertions)]
            return component.inner.downcast_mut::<T>();
            #[cfg(not(debug_assertions))]
            return Some(unsafe { component.inner.downcast_mut_unchecked::<T>() });
        } else {
            None
        }
    }
}

pub struct ComponentBase {
    inner: Box<dyn Any>,
}
impl ComponentBase {}

pub trait Component: Sized + 'static {
    const UUID: Uuid;
    const NAME: &'static str;

    fn create_component_base(self) -> ComponentBase {
        ComponentBase {
            inner: Box::new(self),
        }
    }
}

pub struct EntityTable {
    entities: BTreeMap<Uuid, UnsafeCell<Entity>>,
    component_entity_map: BTreeMap<Uuid, BTreeSet<Uuid>>,
    delete_entities: BTreeMap<Uuid, UnsafeCell<Entity>>,
}

impl EntityTable {
    pub fn new() -> Self {
        Self {
            entities: BTreeMap::new(),
            component_entity_map: BTreeMap::new(),
            delete_entities: BTreeMap::new(),
        }
    }

    pub fn add_new_entity(&mut self, entity: Entity) {
        let id = Uuid::new_v4();
        for (comp_id, _) in &entity.components {
            let entry = self
                .component_entity_map
                .entry(*comp_id)
                .or_insert(BTreeSet::new());
            entry.insert(id);
        }
        self.entities.insert(id, UnsafeCell::new(entity));
    }

    pub fn delete_entity(&mut self, uuid: &Uuid) -> bool {
        if let Some(ent) = self.entities.remove(uuid) {
            self.delete_entities.insert(*uuid, ent);
            true
        } else {
            false
        }
    }

    pub fn all(&self) -> Vec<&Entity> {
        self.entities
            .values()
            .map(|v| unsafe { &*v.get() })
            .collect()
    }
    pub fn all_mut(&mut self) -> Vec<&mut Entity> {
        self.entities.values_mut().map(|v| v.get_mut()).collect()
    }
    pub fn all_delete(&self) -> Vec<&Entity> {
        self.delete_entities
            .values()
            .map(|v| unsafe { &*v.get() })
            .collect()
    }
    pub fn all_delete_mut(&mut self) -> Vec<&mut Entity> {
        self.delete_entities
            .values_mut()
            .map(|v| v.get_mut())
            .collect()
    }

    pub fn get_by_component<T: Component + 'static>(&self) -> Option<BTreeMap<Uuid, &Entity>> {
        let ent_uuids = self.component_entity_map.get(&T::UUID)?;
        let mut m = BTreeMap::new();
        for id in ent_uuids {
            if let Some(cell) = self.entities.get(id) {
                m.insert(*id, unsafe { &*cell.get() });
            }
        }
        Some(m)
    }
    pub fn get_by_component_mut<T: Component + 'static>(
        &mut self,
    ) -> Option<BTreeMap<Uuid, &mut Entity>> {
        let ent_uuids = self.component_entity_map.get(&T::UUID)?;
        let mut m = BTreeMap::new();
        for id in ent_uuids {
            if let Some(cell) = self.entities.get(id) {
                m.insert(*id, unsafe { &mut *cell.get() });
            }
        }
        Some(m)
    }
    pub fn get_by_component_delete<T: Component + 'static>(
        &self,
    ) -> Option<BTreeMap<Uuid, &Entity>> {
        let ent_uuids = self.component_entity_map.get(&T::UUID)?;
        let mut m = BTreeMap::new();
        for id in ent_uuids {
            if let Some(cell) = self.delete_entities.get(id) {
                m.insert(*id, unsafe { &*cell.get() });
            }
        }
        Some(m)
    }
    pub fn get_by_component_delete_mut<T: Component + 'static>(
        &mut self,
    ) -> Option<BTreeMap<Uuid, &mut Entity>> {
        let ent_uuids = self.component_entity_map.get(&T::UUID)?;
        let mut m = BTreeMap::new();
        for id in ent_uuids {
            if let Some(cell) = self.delete_entities.get(id) {
                m.insert(*id, unsafe { &mut *cell.get() });
            }
        }
        Some(m)
    }

    pub fn get_by_uuid(&self, uuid: &Uuid) -> Option<&Entity> {
        self.entities.get(uuid).map(|e| unsafe { &*e.get() })
    }

    pub fn get_by_uuid_mut(&mut self, uuid: &Uuid) -> Option<&mut Entity> {
        self.entities.get_mut(uuid).map(|e| e.get_mut())
    }
}
