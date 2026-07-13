pipeline {
  agent none

  options {
    timestamps()
    disableConcurrentBuilds()
    buildDiscarder(logRotator(numToKeepStr: '20', artifactNumToKeepStr: '10'))
  }

  parameters {
    string(
      name: 'VERSION',
      defaultValue: '',
      description: '发布版本号；为空时由 xuwecli 使用 workspace 默认版本。'
    )
    string(
      name: 'BUNDLE_VERSION',
      defaultValue: '',
      description: '同一版本内递增的构建号；为空时使用 Jenkins BUILD_NUMBER。'
    )
    choice(
      name: 'CHANNEL',
      choices: ['stable', 'beta', 'nightly'],
      description: '自动更新通道。'
    )
    booleanParam(
      name: 'BUILD_MACOS',
      defaultValue: true,
      description: '是否构建 macOS 产物。'
    )
    booleanParam(
      name: 'BUILD_WINDOWS',
      defaultValue: false,
      description: '是否构建 Windows 产物；当前需要等待 xuwecli Windows 打包能力补齐。'
    )
    booleanParam(
      name: 'PUBLISH',
      defaultValue: false,
      description: '是否执行发布上传；当前默认关闭，等 xuwecli publish 接入后再打开。'
    )
  }

  stages {
    stage('macOS') {
      when {
        expression { return params.BUILD_MACOS }
      }
      agent {
        label 'macos-arm64'
      }
      steps {
        checkout scm
        sh '''
          set -eu

          cargo install --path apps/cli --locked

          VERSION_ARG=""
          if [ -n "${VERSION}" ]; then
            VERSION_ARG="--app-version ${VERSION}"
          fi

          BUILD_BUNDLE_VERSION="${BUNDLE_VERSION:-${BUILD_NUMBER}}"

          xuwecli build \
            --targets current \
            ${VERSION_ARG} \
            --bundle-version "${BUILD_BUNDLE_VERSION}" \
            --channel "${CHANNEL}"
        '''
        stash name: 'macos-dist', includes: 'dist/**', allowEmpty: false
        archiveArtifacts artifacts: 'dist/**', fingerprint: true
      }
    }

    stage('Windows') {
      when {
        expression { return params.BUILD_WINDOWS }
      }
      agent {
        label 'windows-x64'
      }
      steps {
        checkout scm
        powershell '''
          $ErrorActionPreference = "Stop"

          cargo install --path apps/cli --locked

          $versionArg = @()
          if ($env:VERSION) {
            $versionArg = @("--app-version", $env:VERSION)
          }

          $buildBundleVersion = $env:BUNDLE_VERSION
          if (-not $buildBundleVersion) {
            $buildBundleVersion = $env:BUILD_NUMBER
          }

          xuwecli build `
            --targets current `
            @versionArg `
            --bundle-version "$buildBundleVersion" `
            --channel "$env:CHANNEL"
        '''
        stash name: 'windows-dist', includes: 'dist/**', allowEmpty: false
        archiveArtifacts artifacts: 'dist/**', fingerprint: true
      }
    }

    stage('Collect') {
      agent {
        label 'macos-arm64'
      }
      steps {
        deleteDir()
        script {
          if (params.BUILD_MACOS) {
            unstash 'macos-dist'
          }
          if (params.BUILD_WINDOWS) {
            unstash 'windows-dist'
          }
        }
        archiveArtifacts artifacts: 'dist/**', fingerprint: true, allowEmptyArchive: true
      }
    }

    stage('Publish') {
      when {
        expression { return params.PUBLISH }
      }
      agent {
        label 'macos-arm64'
      }
      steps {
        sh '''
          set -eu

          echo "xuwecli publish 尚未接入，当前只归档 dist 产物。"
          echo "后续接入后应先上传安装包、校验文件和 notes，最后上传 latest.json。"
        '''
      }
    }
  }

  post {
    always {
      echo "Pipeline finished: ${currentBuild.currentResult}"
    }
  }
}
