//! Who is in each room: people with the party app open, counted from
//! their live-update connections. One person with two tabs counts once.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Presence {
    /// Room, then person, then how many connections they have open.
    rooms: Mutex<HashMap<i64, HashMap<i64, usize>>>,
}

/// Held by a person's live connection; the person leaves when it drops.
pub struct Here {
    presence: Arc<Presence>,
    room: i64,
    user: i64,
}

impl Presence {
    pub fn arrive(self: &Arc<Self>, room: i64, user: i64) -> Here {
        if let Ok(mut rooms) = self.rooms.lock() {
            *rooms.entry(room).or_default().entry(user).or_insert(0) += 1;
        }
        Here {
            presence: Arc::clone(self),
            room,
            user,
        }
    }

    /// How many different people have this room open.
    pub fn count(&self, room: i64) -> usize {
        self.rooms
            .lock()
            .ok()
            .and_then(|rooms| rooms.get(&room).map(HashMap::len))
            .unwrap_or(0)
    }
}

impl Drop for Here {
    fn drop(&mut self) {
        let Ok(mut rooms) = self.presence.rooms.lock() else {
            return;
        };
        if let Some(people) = rooms.get_mut(&self.room) {
            if let Some(open) = people.get_mut(&self.user) {
                *open = open.saturating_sub(1);
                if *open == 0 {
                    people.remove(&self.user);
                }
            }
            if people.is_empty() {
                rooms.remove(&self.room);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_person_with_two_tabs_counts_once_and_leaves_with_the_last() {
        let presence = Arc::new(Presence::default());
        let first = presence.arrive(1, 7);
        let second = presence.arrive(1, 7);
        let other = presence.arrive(1, 8);
        assert_eq!(presence.count(1), 2);
        drop(first);
        assert_eq!(presence.count(1), 2);
        drop(second);
        assert_eq!(presence.count(1), 1);
        drop(other);
        assert_eq!(presence.count(1), 0);
    }

    #[test]
    fn rooms_are_counted_apart() {
        let presence = Arc::new(Presence::default());
        let _a = presence.arrive(1, 7);
        let _b = presence.arrive(2, 8);
        assert_eq!(presence.count(1), 1);
        assert_eq!(presence.count(2), 1);
        assert_eq!(presence.count(3), 0);
    }
}
