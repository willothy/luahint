# luahint

LSP inline hints for Lua, intended for use with Neovim.

Now that inline hints are working in Neovim nightly, I figured I'd attempt to build a LSP-adjacent project. Luahint provides inline parameter hints via LSP, and potentially more in the future. It's a work in progress right now, but here's a screenshot: 

![LuahintDemo](https://github.com/willothy/luahint/assets/38540736/490e4100-914a-4895-95e6-e8c40c85a23f)

# Goals

- [x] Basic function parameter hints
- [ ] Index entire runtime (currently only indexes single file)
- [ ] Table function parameter hints
- [ ] Method parameter hints
- [ ] Metamethod parameter hints
- [ ] Emmylua type hints
