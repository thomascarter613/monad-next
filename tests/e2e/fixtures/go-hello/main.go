// Minimal Go entry point for the monad e2e harness. Kept tiny on
// purpose: the harness asserts monad's init / ci / cache behaviour
// end-to-end, not Go the language.
package main

import "fmt"

func main() {
	fmt.Println(Greeting("monad"))
}

// Greeting builds a canonical hello string. The separate function
// gives `go test` something non-trivial to exercise.
func Greeting(name string) string {
	return fmt.Sprintf("hello, %s", name)
}
