//! A table's groups (RULE-118), kept as the bytes the image frames them
//! in (RULE-45), with the place of each group among them.
//!
//! A group is a count byte and that many command frames: a type, a count
//! of bindings, a 16-bit body length, the body exactly as the server sends
//! that record, and the bindings of 3 bytes each. The bytes are checked
//! when the table is decoded (RULE-70) and only read when a group runs.

use super::Fault;
use crate::bounded::Bounded;
use crate::command::{BINDING_BYTES, Binding};
use crate::limits::{MAX_COMMAND_BINDINGS, MAX_COMMAND_BYTES, MAX_GROUP_COMMANDS, MAX_GROUPS, MAX_GROUPS_BYTES};
use crate::wire::{Reader, Writer};

/// The bytes of a command frame before its body (RULE-45): the type, the
/// count of bindings and the body's length.
pub(crate) const FRAME_FIXED: usize = 1 + 1 + 2;

/// A command to put in a group (RULE-45): a record's type and body, and
/// the bindings of its fields (RULE-106).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Framed<'a> {
    /// The record type (RULE-119).
    pub kind: u8,
    /// The record's body, exactly as the server sends that record.
    pub body: &'a [u8],
    /// The bindings, at most `MAX_COMMAND_BINDINGS`.
    pub bindings: &'a [Binding],
}

/// One command of a group, as the table holds it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CommandRef<'a> {
    /// The record type.
    pub kind: u8,
    /// The record's body.
    pub body: &'a [u8],
    bindings: &'a [u8],
}

impl CommandRef<'_> {
    /// The command's bindings, in written order.
    pub fn bindings(&self) -> impl Iterator<Item = Binding> + '_ {
        self.bindings.as_chunks::<BINDING_BYTES>().0.iter().map(|[item, field, source]| Binding {
            item: *item,
            field: *field,
            source: *source,
        })
    }

    /// How many bindings it has.
    #[must_use]
    pub fn binding_count(&self) -> usize {
        self.bindings.len().checked_div(BINDING_BYTES).unwrap_or(0)
    }
}

/// Where a group's bytes begin, and how many commands it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Entry {
    /// The place of the group's count byte.
    offset: u16,
    /// Its commands.
    commands: u8,
}

/// The groups of a table (RULE-118), numbered from 0.
#[derive(Clone)]
pub struct Groups {
    bytes: [u8; MAX_GROUPS_BYTES],
    used: usize,
    entries: Bounded<Entry, MAX_GROUPS>,
}

impl Groups {
    /// No group.
    #[must_use]
    pub const fn new() -> Self {
        Self { bytes: [0; MAX_GROUPS_BYTES], used: 0, entries: Bounded::new(Entry { offset: 0, commands: 0 }) }
    }

    /// How many groups there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is no group.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The groups' bytes together, as the image frames them (RULE-45).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.bytes.get(..self.used).unwrap_or(&[])
    }

    /// Forgets every group.
    pub fn clear(&mut self) {
        self.used = 0;
        self.entries.clear();
    }

    /// How many commands group `group` holds; 0 for one there is not.
    #[must_use]
    pub fn command_count(&self, group: usize) -> usize {
        self.entries.get(group).map_or(0, |entry| usize::from(entry.commands))
    }

    /// The commands of group `group`, in written order; none for a group
    /// there is not.
    #[must_use]
    pub fn commands(&self, group: usize) -> Commands<'_> {
        let (at, left) = self.start(group).unwrap_or((0, 0));
        Commands { groups: self, at, left }
    }

    /// Where group `group`'s first command frame begins, and how many
    /// commands it holds; `None` for a group there is not.
    #[must_use]
    pub fn start(&self, group: usize) -> Option<(usize, usize)> {
        // Past the group's count byte.
        self.entries.get(group).map(|entry| (usize::from(entry.offset).saturating_add(1), usize::from(entry.commands)))
    }

    /// The command whose frame begins at `at`, and where the next frame
    /// begins.
    #[must_use]
    pub fn frame(&self, at: usize) -> Option<(CommandRef<'_>, usize)> {
        let mut reader = Reader::new(self.as_bytes().get(at..)?);
        let kind = reader.u8().ok()?;
        let bindings = usize::from(reader.u8().ok()?);
        let length = usize::from(reader.u16().ok()?);
        let body = reader.take(length).ok()?;
        let bindings = reader.take(bindings.saturating_mul(BINDING_BYTES)).ok()?;
        Some((CommandRef { kind, body, bindings }, at.saturating_add(reader.used())))
    }

    /// Command `place` of group `group`, if there is one.
    #[must_use]
    pub fn command(&self, group: usize, place: usize) -> Option<CommandRef<'_>> {
        self.commands(group).nth(place)
    }

    /// Adds a group of `commands`, and gives its number.
    pub(super) fn push(&mut self, commands: &[Framed<'_>]) -> Result<u8, Fault> {
        if commands.is_empty() {
            return Err(Fault::Range("group with no command"));
        }
        let count = u8::try_from(commands.len()).ok().filter(|count| usize::from(*count) <= MAX_GROUP_COMMANDS);
        let count = count.ok_or(Fault::Limit("MAX_GROUP_COMMANDS"))?;
        let offset = self.used;
        let mut writer = Writer::new(self.bytes.get_mut(offset..).unwrap_or(&mut []));
        let over = |fault: Fault| if fault == Fault::Full { Fault::Limit("MAX_GROUPS_BYTES") } else { fault };
        writer.u8(count).map_err(over)?;
        for command in commands {
            write_frame(command, &mut writer).map_err(over)?;
        }
        let used = writer.used();
        let entry =
            Entry { offset: u16::try_from(offset).map_err(|_| Fault::Limit("MAX_GROUPS_BYTES"))?, commands: count };
        let number = self.entries.push(entry).map_err(|_| Fault::Limit("MAX_GROUPS"))?;
        self.used = offset.saturating_add(used);
        u8::try_from(number).map_err(|_| Fault::Limit("MAX_GROUPS"))
    }

    /// Reads `count` groups from `reader`, which must take exactly
    /// `bytes` bytes (RULE-41's header field), and keeps them.
    pub(super) fn read(&mut self, reader: &mut Reader<'_>, count: usize, bytes: usize) -> Result<(), Fault> {
        self.clear();
        if bytes > MAX_GROUPS_BYTES {
            return Err(Fault::Limit("MAX_GROUPS_BYTES"));
        }
        let taken = reader.take(bytes)?;
        let mut inner = Reader::new(taken);
        for _ in 0..count {
            let offset = inner.used();
            let commands = inner.u8()?;
            if commands == 0 {
                return Err(Fault::Range("group with no command"));
            }
            if usize::from(commands) > MAX_GROUP_COMMANDS {
                return Err(Fault::Limit("MAX_GROUP_COMMANDS"));
            }
            for _ in 0..commands {
                skip_frame(&mut inner)?;
            }
            let entry =
                Entry { offset: u16::try_from(offset).map_err(|_| Fault::Limit("MAX_GROUPS_BYTES"))?, commands };
            self.entries.push(entry).map_err(|_| Fault::Limit("MAX_GROUPS"))?;
        }
        if !inner.finished() {
            return Err(Fault::Range("bytes of the groups"));
        }
        for (to, from) in self.bytes.iter_mut().zip(taken) {
            *to = *from;
        }
        self.used = taken.len();
        Ok(())
    }
}

impl Default for Groups {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Groups {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes() && self.entries == other.entries
    }
}

impl Eq for Groups {}

impl core::fmt::Debug for Groups {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut list = formatter.debug_list();
        for group in 0..self.len() {
            for command in self.commands(group) {
                list.entry(&(group, command.kind, command.body, command.bindings));
            }
        }
        list.finish()
    }
}

/// Writes one command frame (RULE-45).
fn write_frame(command: &Framed<'_>, writer: &mut Writer<'_>) -> Result<(), Fault> {
    if command.body.is_empty() {
        return Err(Fault::Range("command with no body"));
    }
    if command.body.len() > MAX_COMMAND_BYTES {
        return Err(Fault::Limit("MAX_COMMAND_BYTES"));
    }
    if command.bindings.len() > MAX_COMMAND_BINDINGS {
        return Err(Fault::Limit("MAX_COMMAND_BINDINGS"));
    }
    writer.u8(command.kind)?;
    writer.u8(u8::try_from(command.bindings.len()).map_err(|_| Fault::Limit("MAX_COMMAND_BINDINGS"))?)?;
    writer.u16(u16::try_from(command.body.len()).map_err(|_| Fault::Limit("MAX_COMMAND_BYTES"))?)?;
    writer.bytes(command.body)?;
    for binding in command.bindings {
        writer.bytes(&[binding.item, binding.field, binding.source])?;
    }
    Ok(())
}

/// Passes over one command frame, holding its counts to their limits
/// (RULE-45).
fn skip_frame(reader: &mut Reader<'_>) -> Result<(), Fault> {
    let _kind = reader.u8()?;
    let bindings = usize::from(reader.u8()?);
    if bindings > MAX_COMMAND_BINDINGS {
        return Err(Fault::Limit("MAX_COMMAND_BINDINGS"));
    }
    let length = usize::from(reader.u16()?);
    if length == 0 {
        return Err(Fault::Range("command with no body"));
    }
    if length > MAX_COMMAND_BYTES {
        return Err(Fault::Limit("MAX_COMMAND_BYTES"));
    }
    reader.take(length)?;
    reader.take(bindings.saturating_mul(BINDING_BYTES))?;
    Ok(())
}

/// The commands of one group, in written order.
#[derive(Clone, Debug)]
pub struct Commands<'a> {
    groups: &'a Groups,
    at: usize,
    left: usize,
}

impl<'a> Iterator for Commands<'a> {
    type Item = CommandRef<'a>;

    fn next(&mut self) -> Option<CommandRef<'a>> {
        self.left = self.left.checked_sub(1)?;
        let (command, next) = self.groups.frame(self.at)?;
        self.at = next;
        Some(command)
    }
}
