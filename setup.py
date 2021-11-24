from setuptools import find_packages
from setuptools import setup

setup(
    name='xlslang',
    version='0.1.0',
    license='MIT',
    author='Edward Villegas-Pulgarin',
    author_email='cosmoscalibur@gmail.com',
    packages=find_packages('src'),
    package_dir={'': 'src'},
)