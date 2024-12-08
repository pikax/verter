# What is Verter?

Verter implements Language Server Protocol (LSP), adding support for `.vue` files.

There's only support for Vue 3, Script Setup receives more love but options API will still be supported.

# Why?

Since Vetur times, Vue always had issues with typesafety and the tooling wasn't very good. 

Since Vue 3 and Volar, things got much better. but things are still not perfect.

# Issues trying to fix

I want to improve type safety in SFC also allowing SFC to be used in JSX/TSX.

Strict first approach.

# Approach

Verter uses an approach similar to Svelte, by converting SFC into a valid TSX file, then relying on Typescript to do the heavy lifting, since the generated TSX is valid, there's in theory not much extra work that needs to be done, aside from generating a good TSX.

This approach adds some extra challenges because the user might create SFC in javascript, typescript, etc.

# Differences between Verter and Volar?

Verter is experimental and Volar is very mature, if you haven't encounter any particular issues with Volar, there's no reason to change. 

Volar is feature rich, while this project is trying to bring the best typescript to vue.


# Credits

- [Svelte language-tools](https://github.com/sveltejs/language-tools) for proving inspiration.
- [Vetur](https://github.com/vuejs/vetur) for providing the base for languages.
- [Volar](https://github.com/vuejs/language-tools) inspiration and testing.