-- A node's screen now follows its room's display setting unless the node
-- has its own (an empty `screen`). Nodes on the old default, the TV page,
-- follow their room, whose default is the TV page too, so nothing changes
-- on screen.
UPDATE nodes SET screen = '' WHERE screen = 'tv';
