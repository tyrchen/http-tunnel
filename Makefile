.PHONY: help build-lambda build-forwarder test deploy-infra preview-infra destroy-infra clean run-testapp

help:
	@echo "HTTP Tunnel Makefile"
	@echo ""
	@echo "Available targets:"
	@echo "  build-lambda      - Build Lambda handler for AWS (requires cargo-lambda)"
	@echo "  build-forwarder   - Build forwarder binary for local use"
	@echo "  test              - Run all tests"
	@echo "  deploy-infra      - Build Lambda and deploy infrastructure with Pulumi"
	@echo "  preview-infra     - Preview infrastructure changes"
	@echo "  destroy-infra     - Destroy all infrastructure"
	@echo "  run-testapp       - Run the TodoMVC test app on port 3000"
	@echo "  clean             - Clean build artifacts"

# Build Lambda function (requires cargo-lambda)
build-lambda:
	@echo "Building Lambda handler..."
	cargo lambda build --release --arm64 --bin handler
	@echo "Copying Lambda to infra/lambda/handler..."
	@mkdir -p infra/lambda/handler
	@if [ -d "$$HOME/.target/lambda/handler" ]; then \
		cp -r $$HOME/.target/lambda/handler/* infra/lambda/handler/; \
	elif [ -d "target/lambda/handler" ]; then \
		cp -r target/lambda/handler/* infra/lambda/handler/; \
	else \
		echo "Error: Lambda build output not found"; exit 1; \
	fi
	@if [ -f "infra/jwks.json" ]; then \
		cp infra/jwks.json infra/lambda/handler/jwks.json; \
		echo "✓ JWKS file copied to Lambda package"; \
	fi
	@echo "✓ Lambda binary built and copied to infra/lambda/handler/bootstrap"

# Build forwarder for local development
build-forwarder:
	@echo "Building forwarder..."
	cargo build --release --bin ttf
	cargo install --path apps/forwarder

# Run all tests
test:
	@echo "Running tests..."
	cargo test --all

# Deploy infrastructure (builds Lambda first)
deploy-infra: build-lambda
	@echo "Deploying infrastructure with Pulumi..."
	cd infra && pulumi up

# Preview infrastructure changes (builds Lambda first)
preview-infra: build-lambda
	@echo "Previewing infrastructure changes..."
	cd infra && pulumi preview

# Destroy infrastructure
destroy-infra:
	@echo "Destroying infrastructure..."
	cd infra && pulumi destroy

# Run the TodoMVC test app
run-testapp:
	@echo "Starting TodoMVC test app on port 3000..."
	cd testapp && uv run python main.py

publish:
	@echo "Publishing to cargo..."
	cargo publish -p http-tunnel-common
	cargo publish -p ttf


# Clean build artifacts
clean:
	@echo "Cleaning build artifacts..."
	cargo clean
	rm -rf infra/lambda/handler
	rm -rf infra/node_modules
	rm -rf infra/dist
