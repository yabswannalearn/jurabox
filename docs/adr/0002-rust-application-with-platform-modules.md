---
status: accepted
---

# Start with one Rust application and separate platform integration

Jurabox will begin as one Rust application, with separate modules for project rules, session and service management, the UI, and operating-system integration. Only Windows 11 integration will be implemented initially. This keeps the project model independent of platform-specific desktop behavior so macOS and Linux can be investigated later without building unused integrations now.

A collection of services or language runtimes would add communication, startup, packaging, and failure-recovery work without a demonstrated benefit for this local desktop application. A helper process or another language remains an option when a concrete library or isolation requirement justifies it. The final UI toolkit and desktop integration library still require validation.
