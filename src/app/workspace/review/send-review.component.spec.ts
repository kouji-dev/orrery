import { Component, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { SendReviewButtonComponent, SendReviewModalComponent } from "./send-review.component";
import { ReviewStore } from "../../agents/review.store";
import { AgentReviewService } from "../../agents/agent-review.service";
import { IconComponent } from "../../shared/icon.component";

// Stub IconComponent (uses signal input.required — NG0950 under JIT)
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

// Stub AgentReviewService dependency (avoid PTY / network side-effects in unit tests)
class AgentReviewServiceStub { sendReview() {} }

describe("SendReviewButtonComponent", () => {
  let store: ReviewStore;
  beforeEach(() => {
    TestBed.configureTestingModule({
      imports: [SendReviewButtonComponent],
      providers: [
        provideZonelessChangeDetection(),
        { provide: AgentReviewService, useClass: AgentReviewServiceStub },
      ],
    });
    TestBed.overrideComponent(SendReviewButtonComponent, {
      remove: { imports: [IconComponent, SendReviewModalComponent] },
      add: { imports: [IconStub] },
    });
    store = TestBed.inject(ReviewStore);
  });
  afterEach(() => TestBed.resetTestingModule());

  it("hidden at 0, shows count badge once there are comments", () => {
    const f = TestBed.createComponent(SendReviewButtonComponent);
    f.componentRef.setInput("agent", "a"); f.componentRef.setInput("agentName", "Bee");
    f.detectChanges();
    expect(f.nativeElement.querySelector("button")).toBeNull();
    store.add("a", { file: "f", view: "diff", lang: "ts", fromLine: 1, toLine: 1, side: "new", snippet: "s", lines: ["s"], note: "n" });
    f.detectChanges();
    expect(f.nativeElement.textContent).toContain("Send review");
    expect(f.nativeElement.textContent).toContain("1");
  });
});
