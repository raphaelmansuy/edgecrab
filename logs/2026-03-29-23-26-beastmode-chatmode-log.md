Actions: Investigated Signal setup flow, confirmed gateway health/log root cause, hardened Signal wizard link detection, built and tested edgecrab-cli.
Decisions: Removed strict --version gate from Step 3b path and added PATH-based signal-cli presence fallback to avoid Java-related false negatives.
Next steps: User should complete signal-cli link scan on phone, start signal-cli daemon on 127.0.0.1:8090, then restart gateway.
Lessons/insights: Signal failure is operational (account not linked/registered) after SSE endpoint fix; wizard now reliably surfaces link instructions.
