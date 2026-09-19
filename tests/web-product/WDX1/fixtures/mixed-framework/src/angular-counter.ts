import { Component, signal } from "@angular/core";

@Component({
  selector: "app-counter",
  template: '<button type="button" (click)="increment()">{{ count() }}</button>',
})
export class Counter {
  readonly count = signal(0);

  increment(): void {
    this.count.set(this.count() + 1);
  }
}
