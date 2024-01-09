.PHONY: build clean publish

# Variables
DOCKER_IMAGE_NAME ?= gallynaut/solana-randomness-service
VERSION := $(shell cat ../../../../../version)

# Default make task
all: build

anchor_init:
	@echo "\n\033[1;34mAnchor init ...\033[0m"  # Blue text
	@anchor keys sync
	$(eval PUBKEY=$(shell solana-keygen pubkey target/deploy/solana_randomness_service-keypair.json))
	@echo "\033[1;34mChecking randomness service IDL ${PUBKEY} ...\033[0m"  # Blue text
	@anchor idl fetch --provider.cluster devnet ${PUBKEY} > /dev/null 2>&1 || \
		anchor idl init --provider.cluster devnet -f target/idl/solana_randomness_service.json ${PUBKEY}
	$(eval PUBKEY=$(shell solana-keygen pubkey target/deploy/solana_randomness_consumer-keypair.json))
	@echo "\033[1;34mChecking consumer program IDL (${PUBKEY}) ...\033[0m"  # Blue text
	@anchor idl fetch --provider.cluster devnet ${PUBKEY} > /dev/null 2>&1 || \
		anchor idl init --provider.cluster devnet -f target/idl/solana_randomness_consumer.json ${PUBKEY}
	@echo "\033[1;32m✔ Anchor init successful\033[0m"

anchor_build:
	@echo "\n\033[1;34mBuilding anchor projects ...\033[0m"  # Blue text
	@anchor build > /dev/null 2>&1 && \
	(echo "\033[1;32m✔ Anchor build successful\033[0m") || \
	(echo "\033[1;31m✘ Anchor build failed\033[0m" && false)  # Red text on failure

anchor_test:
	@echo "\n\033[1;34mRunning anchor tests ...\033[0m"  # Blue text
	@anchor test --provider.cluster localnet > /dev/null 2>&1 && \
	(echo "\033[1;32m✔ Anchor tests successful\033[0m") || \
	(echo "\033[1;31m✘ Anchor tests failed\033[0m" && false)  # Red text on failure

anchor_deploy: anchor_test
	@echo "\n\033[1;34mDeploying anchor projects ...\033[0m"  # Blue text
	@anchor deploy --provider.cluster devnet && \
	(echo "\033[1;32m✔ Anchor deploy successful\033[0m") || \
	(echo "\033[1;31m✘ Anchor deploy failed\033[0m" && false)  # Red text on failure
	@echo "\n\033[1;34mDeploying anchor IDLs ...\033[0m"  # Blue text
	@anchor idl upgrade --provider.cluster devnet && \
	(echo "\033[1;32m✔ Anchor deploy successful\033[0m") || \
	(echo "\033[1;31m✘ Anchor deploy failed\033[0m" && false)  # Red text on failure

docker_build: anchor_build
	@echo "\n\033[1;34mBuilding ${DOCKER_IMAGE_NAME}:dev-${VERSION} ...\033[0m"  # Blue text
	@docker buildx build -f Dockerfile --platform linux/amd64 --tag ${DOCKER_IMAGE_NAME}:dev-${VERSION} --pull --load ../../../../../ && \
	(echo "\033[1;32m✔ Docker build successful\033[0m") || \
	(echo "\033[1;31m✘ Docker build failed\033[0m" && false)  # Red text on failure

build: docker_build

docker_publish: anchor_test
	@echo "\n\033[1;34mPublishing ${DOCKER_IMAGE_NAME}:dev-${VERSION} ...\033[0m"  # Blue text
	@docker buildx build -f Dockerfile --platform linux/amd64 --tag ${DOCKER_IMAGE_NAME}:dev-${VERSION} --pull --push ../../../../../ && \
	(docker image pull --platform=linux/amd64 ${DOCKER_IMAGE_NAME}:dev-${VERSION} > /dev/null 2>&1) && \
	(echo "\033[1;32m✔ Publish successful\033[0m") || \
	(echo "\033[1;31m✘ Publish failed\033[0m" && false)  # Red text on failure
	@make measurement

publish: docker_publish

measurement:
	@rm -rf measurement.txt || true
	@docker rm my-switchboard-function > /dev/null 2>&1 || true
	@echo "\033[1;34mRunning measurement...\033[0m"  # Blue text
	@docker run -d --platform=linux/amd64 -q --name=my-switchboard-function ${DOCKER_IMAGE_NAME}:dev-${VERSION} > /dev/null 2>&1
	@docker cp my-switchboard-function:/measurement.txt measurement.txt  > /dev/null 2>&1
	$(eval MEASUREMENT=$(shell awk '/Measurement:/ { getline; print $$1 }' measurement.txt))
	@echo -n "\033[1;34mMrEnclve: \033[0m${MEASUREMENT}\n"  # Blue text
	@docker stop my-switchboard-function > /dev/null 2>&1 || true
	@docker rm my-switchboard-function > /dev/null 2>&1 || true
	@echo "\033[1;34mTagging Docker image...\033[0m"  # Blue text
	@docker tag ${DOCKER_IMAGE_NAME}:dev-${VERSION} ${DOCKER_IMAGE_NAME}:${MEASUREMENT} && \
	(echo "\033[1;32m✔ Docker image tagged successfully\033[0m") || \
	(echo "\033[1;31m✘ Docker image tagging failed\033[0m" && false)  # Red text on failure
	@echo "\033[1;34mPushing Docker image...\033[0m"  # Blue text
	@docker push ${DOCKER_IMAGE_NAME}:${MEASUREMENT} && \
	(echo "\033[1;32m✔ Docker image pushed successfully\033[0m") || \
	(echo "\033[1;31m✘ Docker image push failed\033[0m" && false)  # Red text on failure

# Task to clean up the compiled rust application
clean:
	@echo "\n\033[1;34mCleaning up ...\033[0m"  # Blue text
	@cargo clean && \
	(echo "\033[1;32m✔ Clean up successful\033[0m") || \
	(echo "\033[1;31m✘ Clean up failed\033[0m" && false)  # Red text on failure
