-- The addresses a node says it has on its own network (one per line), kept
-- beside `address`, which is only where its check-ins seem to come from. A
-- subnet router, NAT or VPN in between changes that one, so the link that
-- opens the node's page tries these first.
ALTER TABLE nodes ADD COLUMN addresses TEXT NOT NULL DEFAULT '';
