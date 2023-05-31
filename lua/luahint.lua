local M = {}

---@class State
---@field enabled boolean
---@field config Options
local state = {}

---@class Options Configuration options
---@field update? string[] | string Autocommands that should trigger a refresh
---@field root_dir? fun(): string Function to determine the root directory of the project
---@field enabled_at_startup? boolean Whether to enable the plugin at startup
local default = {
	---@type string | string[]
	update = {
		"CursorHold",
		"CursorHoldI",
		"InsertLeave",
		"TextChanged",
	},
	enabled_at_startup = true,
	root_dir = vim.fn.getcwd,
}

---@type number Namespace id
local namespace = vim.api.nvim_create_namespace("luahint")

---@param id number LSP Client ID
---@param buf number Buffer number
local function fetch_hints(id, buf)
	local params = vim.lsp.util.make_range_params(0, "utf-8")

	local client = vim.lsp.get_client_by_id(id)

	if not client then
		return
	end

	local handler = function(e, res)
		if res and not e then
			for i, hint in ipairs(res) do
				local opts = {
					id = i,
					virt_text = { { hint.kind == 1 and (": " .. hint.label) or (hint.label .. ": "), "LspInlayHint" } },
					virt_text_pos = "inline",
				}
				vim.api.nvim_buf_set_extmark(
					buf or 0,
					namespace,
					hint.position.line - 1,
					hint.position.character - 1,
					opts
				)
			end
		end
	end

	client.request("textDocument/inlayHint", params, handler, buf or 0)
end

function M.show()
	state.enabled = true
end

function M.hide()
	state.enabled = false
	vim.api.nvim_buf_clear_namespace(0, namespace, 0, -1)
end

function M.toggle()
	state.enabled = not state.enabled
	if not state.enabled then
		vim.api.nvim_buf_clear_namespace(0, namespace, 0, -1)
	end
end

---@param opts Options | nil
function M.setup(opts)
	opts = vim.tbl_deep_extend("keep", opts or {}, default)
	if type(opts.update) == "string" then
		opts.update = { opts.update }
	end

	state.enabled = opts.enabled_at_startup
	state.config = opts

	if state.autocmd ~= nil then
		-- user is calling setup again, remove the old autocmd
		vim.api.nvim_del_autocmd(state.autocmd)
	end

	state.autocmd = vim.api.nvim_create_autocmd("FileType", {
		pattern = "lua",
		callback = function(ev)
			local buf = ev.buf

			local client_id = vim.lsp.start({
				name = "luahint",
				cmd = { "luahint" },
				root_dir = opts.root_dir(),
			})
			vim.lsp.buf_attach_client(buf, client_id)

			-- events when inlay hints should update
			vim.api.nvim_create_autocmd(opts.update, {
				buffer = buf,
				callback = function(e)
					if state.enabled then
						fetch_hints(client_id, e.buf)
					end
				end,
			})
		end,
	})
end

return M
