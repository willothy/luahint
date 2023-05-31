# luahint

LSP inline hints for Lua, intended for use with Neovim.

Now that inline hints are working in Neovim nightly, I figured I'd attempt to build a LSP-adjacent project. Luahint provides inline parameter hints via LSP, and potentially more in the future. It's a work in progress right now, but here's a screenshot:

![LuahintDemo](https://github.com/willothy/luahint/assets/38540736/490e4100-914a-4895-95e6-e8c40c85a23f)

## Goals

- [x] Basic function parameter hints
- [ ] Index entire runtime (currently only indexes single file)
- [ ] Table function parameter hints
- [ ] Method parameter hints
- [ ] Metamethod parameter hints
- [ ] Emmylua type hints

## Installation

With `lazy.nvim`

```lua
{
	"willothy/luahint",
	build = "cargo install --path=./",
	config = true
	-- or opts = { ... }
}
```

## Configuration

Luahint comes with the following defaults:

```lua
{
	-- string[] | string
	-- Autocommands that should trigger a refresh
	update = {
		"CursorHold",
		"CursorHoldI",
		"InsertLeave",
		"TextChanged",
	},

	-- boolean
	-- Whether to enable the plugin at startup
	enabled_at_startup = true,

	-- fun(): string
	-- Function to determine the root directory of the project
	root_dir = vim.fn.getcwd,
}
```

## Usage

```lua
local hints = require("luahint")

-- show the hints
hints.show()

-- hide the hints
hints.hide()

-- toggle the hints
hints.toggle()

```
